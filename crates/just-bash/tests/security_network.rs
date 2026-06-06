use std::cell::RefCell;
use std::collections::BTreeMap;

use just_bash::security::ExecutionLimits;
use just_bash::{
    AllowedUrlEntry, Bash, BashOptions, CancellationState, CommandSecurityPolicy, DnsAddress,
    DnsLookupError, HttpMethod, JUST_BASH_BACKEND, JustBashExecOptions, JustBashSession,
    JustBashSessionOptions, NetworkPolicy, NetworkRequest, NetworkResponse, ResourceObservation,
    SecurityDiagnostic, SecurityDiagnosticCode, SecurityViolationLog, StaticNetworkTransport,
    UpstreamRuntimeSurface, classify_runtime_surface, execute_network_request, is_private_hostname,
    is_url_allowed, matches_allow_list_entry, plan_network_request, validate_allow_list_entry,
    validate_workspace_path,
};
use url::Url;

fn combined_output(result: &just_bash::JustBashExecResult) -> String {
    format!("{}{}", result.stdout, result.stderr)
}

fn assert_no_host_or_secret_markers(output: &str) {
    for marker in [
        "root:x:",
        "/bin/bash",
        "/Users/",
        "/private/",
        "node:internal",
        "file://",
        "AWS_SECRET",
        "AWS_ACCESS_KEY",
        "GITHUB_TOKEN",
        "API_KEY",
        "PRIVATE_KEY",
        "sensitive_data_12345",
        "[native code]",
        "process.env",
    ] {
        assert!(
            !output.contains(marker),
            "unexpected sensitive marker {marker:?} in output: {output:?}"
        );
    }
}

fn public_dns(host: &str) -> Result<Vec<DnsAddress>, DnsLookupError> {
    let address = match host {
        "first.example" => "8.8.8.8",
        "second.example" => "1.1.1.1",
        _ => "93.184.216.34",
    };
    Ok(vec![DnsAddress::new(address, 4)])
}

#[test]
fn just_bash_security_allow_deny_policy_blocks_denied_and_unknown_commands() {
    let policy = CommandSecurityPolicy::allow_only(["echo", "cat"]).deny("curl");

    assert!(policy.check_command("echo hello").is_ok());

    let denied = policy
        .check_command("curl https://example.com")
        .unwrap_err();
    assert_eq!(denied.code, SecurityDiagnosticCode::CommandDenied);
    assert!(denied.message.contains("curl"));

    let unknown = policy.check_command("rm -rf /").unwrap_err();
    assert_eq!(unknown.code, SecurityDiagnosticCode::CommandNotAllowed);
    assert!(unknown.message.contains("rm"));
}

#[test]
fn just_bash_security_redacts_sandbox_paths_and_sensitive_env_values() {
    let policy = just_bash::RedactionPolicy::new().with_sandbox_root("/private/tmp/sandbox");
    let env = [
        ("API_TOKEN", "secret-token-123"),
        ("NORMAL", "visible"),
        ("PRIVATE_KEY_PATH", "/private/tmp/sandbox/key.pem"),
    ];

    let message = policy.redact_message(
        "failed at /private/tmp/sandbox/src/main.rs with token secret-token-123",
        env.iter().copied(),
    );
    assert_eq!(
        message,
        "failed at <sandbox>/src/main.rs with token <redacted>"
    );

    let (env_snapshot, diagnostics) = policy.redact_env(env.iter().copied());
    assert_eq!(env_snapshot["API_TOKEN"], "<redacted>");
    assert_eq!(env_snapshot["NORMAL"], "visible");
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == SecurityDiagnosticCode::SensitiveValueRedacted)
    );
}

#[test]
fn just_bash_security_path_validation_blocks_escape_and_nul_cases() {
    assert_eq!(
        validate_workspace_path("safe/nested.txt").expect("valid path"),
        "safe/nested.txt"
    );
    assert_eq!(
        validate_workspace_path("../etc/passwd").unwrap_err().code,
        SecurityDiagnosticCode::PathEscape
    );
    assert_eq!(
        validate_workspace_path("safe\0name").unwrap_err().code,
        SecurityDiagnosticCode::PathContainsNul
    );
}

#[test]
fn just_bash_security_sandbox_command_rows_are_virtual_and_registry_bound() {
    let bash = Bash::new();

    let remove_file = bash.exec(
        r#"
        echo "content" > /tmp/testfile.txt
        cat /tmp/testfile.txt
        rm /tmp/testfile.txt
        cat /tmp/testfile.txt 2>&1 || echo "deleted"
        "#,
    );
    assert_eq!(remove_file.exit_code, 0);
    assert!(remove_file.stdout.contains("content"));
    assert!(remove_file.stdout.contains("deleted"));
    assert!(!bash.file_exists("/tmp/testfile.txt"));

    let remove_tree = bash.exec(
        r#"
        mkdir -p /tmp/testdir/subdir
        echo "file" > /tmp/testdir/subdir/file.txt
        rm -rf /tmp/testdir
        ls /tmp/testdir 2>&1 || echo "removed"
        "#,
    );
    assert_eq!(remove_tree.exit_code, 0);
    assert!(remove_tree.stdout.contains("removed"));
    assert!(!bash.file_exists("/tmp/testdir/subdir/file.txt"));

    let copy_move = bash.exec(
        r#"
        echo "content" > /tmp/cptest1.txt
        cp /tmp/cptest1.txt /tmp/cptest2.txt
        mv /tmp/cptest2.txt /tmp/mvtest2.txt
        cat /tmp/mvtest2.txt
        "#,
    );
    assert_eq!(copy_move.exit_code, 0);
    assert_eq!(copy_move.stdout, "content\n");
    assert_eq!(bash.read_file("/tmp/cptest1.txt").unwrap(), "content\n");

    let restricted = Bash::with_options(BashOptions {
        commands: Some(vec!["echo".to_string()]),
        ..BashOptions::default()
    });
    let path_hijack = restricted.exec(
        r#"
        export PATH=/tmp
        /bin/ls 2>&1 || echo "registry-only"
        "#,
    );
    assert_eq!(path_hijack.exit_code, 0);
    assert!(path_hijack.stdout.contains("registry-only"));
    // `/bin/ls` is an absolute path to a file that does not exist in the virtual
    // filesystem (and `ls` is not in this restricted registry), so it fails
    // closed with "No such file or directory" — matching upstream
    // `resolveCommand`, which reports a missing explicit path distinctly from a
    // bare-name PATH miss. A registered `$PATH` directory hijack cannot smuggle
    // in a host binary.
    assert!(
        path_hijack
            .stdout
            .contains("/bin/ls: No such file or directory"),
        "stdout={}",
        path_hijack.stdout
    );

    let unknown = bash.exec("nonexistent_command 2>&1 || echo not-found");
    assert_eq!(unknown.exit_code, 0);
    assert!(unknown.stdout.contains("not-found"));
    assert!(
        unknown
            .stdout
            .contains("nonexistent_command: command not found")
    );

    let bin_ls = bash.exec("/bin/ls /tmp 2>&1");
    assert_eq!(bin_ls.exit_code, 0);
    assert_no_host_or_secret_markers(&combined_output(&bin_ls));

    for command in ["chmod", "chown", "hash", "enable", "jobs", "fg", "bg"] {
        let result = bash.exec(format!("{command} 2>&1 || echo handled"));
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("handled"));
        assert_no_host_or_secret_markers(&combined_output(&result));
    }
}

#[test]
fn just_bash_security_sandbox_dynamic_rows_fail_closed_or_stay_in_process() {
    let bash = Bash::new();

    let nested = bash.exec("bash -c 'echo inner'; sh -c 'echo shell'");
    assert_eq!(nested.exit_code, 0);
    assert_eq!(nested.stdout, "inner\nshell\n");

    let substitution = bash.exec("echo `echo safe`; echo $(echo nested)");
    assert_eq!(substitution.exit_code, 0);
    assert_no_host_or_secret_markers(&combined_output(&substitution));

    for script in [
        "eval 'cat /etc/passwd' 2>&1 || echo blocked",
        "source /etc/profile 2>&1 || echo blocked",
        ". /etc/profile 2>&1 || echo blocked",
        "trap 'cat /etc/shadow' EXIT 2>&1 || echo blocked",
        "alias ls='cat /etc/passwd' 2>&1 || echo blocked",
        "cat <(echo unsafe) 2>&1 || echo blocked",
    ] {
        let result = bash.exec(script);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("blocked"));
        assert_no_host_or_secret_markers(&combined_output(&result));
    }

    let prompt = bash.exec("export PS1='$(cat /etc/passwd)'; echo prompt-set");
    assert_eq!(prompt.exit_code, 0);
    assert_eq!(prompt.stdout, "prompt-set\n");
    assert_no_host_or_secret_markers(&combined_output(&prompt));

    let limited =
        JustBashSession::with_options(JustBashSessionOptions::new().with_max_command_count(2));
    let too_many = limited.exec("echo one; echo two; echo three", JustBashExecOptions::new());
    assert_ne!(too_many.exit_code, 0);
    assert!(too_many.stderr.contains("maximum command count"));
    assert!(!too_many.stdout.contains("three"));
}

#[test]
fn just_bash_security_sandbox_information_disclosure_rows_do_not_expose_host_state() {
    let bash = Bash::new();

    for script in [
        "invalid_syntax_here()))) 2>&1 || true",
        "cat /nonexistent/path/file.txt 2>&1 || true",
        "ls / 2>&1 || true",
        "cat /etc/shadow 2>&1 || true",
        "ls /etc 2>&1 || true",
        "cat /proc/self/environ 2>&1 || true",
        "ls ~ 2>&1 || true",
        "hostname 2>&1 || echo hostname-handled",
        "uname -a 2>&1 || echo uname-handled",
        "id 2>&1 || echo id-handled",
        "ps aux 2>&1 || echo ps-handled",
        "ifconfig 2>&1 || echo ifconfig-handled",
        "netstat 2>&1 || echo netstat-handled",
        "history 2>&1 || echo history-handled",
        "fc -l 2>&1 || echo fc-handled",
        "echo $HISTFILE",
        "echo $HOSTNAME",
        "source /etc/profile 2>&1 || echo source-blocked",
        "cat /dev/mem 2>&1 || true",
    ] {
        let result = bash.exec(script);
        assert_no_host_or_secret_markers(&combined_output(&result));
    }

    let env = bash.exec("env");
    assert_eq!(env.exit_code, 0);
    assert!(env.stdout.lines().count() < 15);
    assert!(env.stdout.contains("HOME=/home/user"));
    assert!(env.stdout.contains("HOSTNAME=localhost"));
    assert_no_host_or_secret_markers(&env.stdout);

    assert_eq!(bash.exec("whoami").stdout, "user\n");

    let first = Bash::new();
    let second = Bash::new();
    assert_eq!(
        first
            .exec("export SECRET=sensitive_data_12345; echo set")
            .stdout,
        "set\n"
    );
    let leaked = second.exec("echo $SECRET");
    assert_eq!(leaked.stdout, "\n");
    assert_no_host_or_secret_markers(&combined_output(&leaked));

    let same_instance = Bash::new();
    assert_eq!(
        same_instance
            .exec("export PERSIST_VAR=value; echo $PERSIST_VAR")
            .stdout,
        "value\n"
    );
    assert_eq!(same_instance.exec("echo $PERSIST_VAR").stdout, "\n");
}

#[test]
fn just_bash_security_fuzzing_attack_oracles_are_ported_to_rust() {
    let bash = Bash::new();
    for attack in [
        "cat /etc/passwd 2>&1 || true",
        "cat ../../../etc/passwd 2>&1 || true",
        "echo $(cat /etc/passwd) 2>&1 || true",
        "source /etc/passwd 2>&1 || true",
        "eval \"cat /etc/passwd\" 2>&1 || true",
        "cat /proc/self/environ 2>&1 || true",
        "ln -s /etc/passwd link && cat link 2>&1 || true",
    ] {
        let result = bash.exec(attack);
        assert_no_host_or_secret_markers(&combined_output(&result));
    }

    let limited_output =
        JustBashSession::with_options(JustBashSessionOptions::new().with_max_output_length(8));
    let output = limited_output.exec("printf '0123456789abcdef'", JustBashExecOptions::new());
    assert_eq!(output.stdout.len(), 8);
    assert!(output.metadata.truncated);

    let policy = CommandSecurityPolicy::allow_only(["echo"]).deny("curl");
    assert!(policy.check_command("echo safe").is_ok());
    assert_eq!(
        policy
            .check_command("curl http://example.com")
            .unwrap_err()
            .code,
        SecurityDiagnosticCode::CommandDenied
    );
}

#[test]
fn just_bash_security_jbc27_attack_corpus_paths_and_injection_rows_are_virtualized() {
    let bash = Bash::new();

    let scripts = [
        r#"
        touch '/tmp/file with spaces.txt'
        cat '/tmp/file with spaces.txt'
        rm '/tmp/file with spaces.txt'
        "#,
        r#"
        touch '/tmp/`echo pwned`.txt'
        cat '/tmp/`echo pwned`.txt'
        rm '/tmp/`echo pwned`.txt'
        "#,
        r#"
        touch '/tmp/$(echo pwned).txt'
        cat '/tmp/$(echo pwned).txt'
        rm '/tmp/$(echo pwned).txt'
        "#,
        r#"
        mkdir -p /tmp/a/b/c
        cd /tmp/a/b/c
        ls ../../../..
        "#,
        "cat /etc/passwd 2>&1 || true",
        "cat ../../../etc/passwd 2>&1 || true",
        "echo '$HOME; cat /etc/passwd'",
        "env 'bad;echo pwned' 2>&1 || echo env-literal",
        "timeout 1 echo safe 2>&1 || echo timeout-unavailable",
    ];

    for script in scripts {
        let result = bash.exec(script);
        assert_no_host_or_secret_markers(&combined_output(&result));
        assert!(
            !result.stdout.lines().any(|line| line == "pwned"),
            "script unexpectedly executed injected payload: {script:?} -> {result:?}"
        );
    }

    let nul = bash.exec("cat \"/etc\0/passwd\" 2>&1 || true");
    assert_no_host_or_secret_markers(&combined_output(&nul));
    assert!(!nul.stdout.contains("root:x:"));
}

#[test]
fn just_bash_security_jbc27_prototype_pollution_keywords_remain_plain_data() {
    let dangerous_keys = [
        "__proto__",
        "constructor",
        "prototype",
        "hasOwnProperty",
        "toString",
        "valueOf",
        "__defineGetter__",
        "__defineSetter__",
        "__lookupGetter__",
        "__lookupSetter__",
    ];

    let base = Bash::with_options(BashOptions {
        env: dangerous_keys
            .iter()
            .map(|key| ((*key).to_string(), format!("value-for-{key}")))
            .collect(),
        ..BashOptions::default()
    });

    for key in dangerous_keys {
        let result = base.exec(format!("printenv {key}"));
        assert_eq!(result.exit_code, 0, "{key}");
        assert_eq!(result.stdout, format!("value-for-{key}\n"), "{key}");
        assert_no_host_or_secret_markers(&combined_output(&result));
    }

    let assignment = Bash::new().exec(
        r#"
        export __proto__=plain
        export constructor=ctor
        printenv __proto__
        printenv constructor
        "#,
    );
    assert_eq!(assignment.exit_code, 0);
    assert_eq!(assignment.stdout, "plain\nctor\n");
    assert_no_host_or_secret_markers(&combined_output(&assignment));

    let jq = Bash::new().exec(
        r#"echo '{"__proto__":{"polluted":true},"constructor":"plain"}' | jq '.__proto__.polluted, .constructor'"#,
    );
    assert_eq!(jq.exit_code, 0);
    assert!(jq.stdout.contains("true"));
    assert!(jq.stdout.contains("\"plain\""));
    assert_no_host_or_secret_markers(&combined_output(&jq));

    let awk = Bash::new().exec(r#"echo "row" | awk -v constructor=data '{ print constructor }'"#);
    assert_eq!(awk.exit_code, 0);
    assert_eq!(awk.stdout, "data\n");
    assert_no_host_or_secret_markers(&combined_output(&awk));
}

#[test]
fn just_bash_security_jbc27_fuzz_oracle_malformed_inputs_never_leak_host_state() {
    let bash = Bash::new();
    let probes = [
        "echo ${#__defineGetter__}",
        "echo ${#__proto__[@]}",
        "__lookupGetter__=safe; constructor=plain; echo $__lookupGetter__ $constructor",
        "echo '{} ' | jq '.__defineGetter__'",
        "echo '{\"a\":{\"__proto__\":1}}' | jq '.. | objects | .__proto__? // empty'",
        "awk -F, '{ XW4pCI93-- }' .. 2>&1 || true",
        "if then",
        "'unterminated",
        "$((1 / 0))",
        "echo $((2147483647))",
        "echo $((-2147483648))",
        "echo $((2147483647 + 1))",
        "echo $((-2147483648 - 1))",
        "echo $((999999999 * 999999999))",
        "echo $((10 % 0))",
        "cat <(echo unsafe) 2>&1 || echo process-substitution-blocked",
    ];

    for probe in probes {
        let result = bash.exec(probe);
        assert_no_host_or_secret_markers(&combined_output(&result));
        assert!(
            !combined_output(&result).contains("[native code]"),
            "probe leaked native code marker: {probe:?} -> {result:?}"
        );
    }
}

#[test]
fn just_bash_security_jbc27_sandbox_facade_rows_use_virtual_session_contract() {
    let session = JustBashSession::with_options(
        JustBashSessionOptions::new()
            .with_cwd("/app")
            .with_env("TOKEN", "sandbox-token")
            .with_file("/app/input.txt", "hello\n"),
    );

    let result = session.exec(
        "pwd; printenv TOKEN; cat input.txt; mkdir -p logs; echo done > logs/out.txt",
        JustBashExecOptions::new(),
    );
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "/app\nsandbox-token\nhello\n");
    assert_eq!(result.metadata.backend, JUST_BASH_BACKEND);
    assert!(!result.metadata.external_sandbox);
    assert_eq!(session.read_file("/app/logs/out.txt").unwrap(), "done\n");
    assert!(session.file_exists("/app/input.txt"));
    assert!(
        session
            .registered_command_names()
            .contains(&"echo".to_string())
    );

    let scoped = session.exec(
        "export TOKEN=changed; printenv TOKEN",
        JustBashExecOptions::new(),
    );
    assert_eq!(scoped.stdout, "changed\n");
    let restored = session.exec("printenv TOKEN", JustBashExecOptions::new());
    assert_eq!(restored.stdout, "sandbox-token\n");

    let timed = JustBashSession::with_options(
        JustBashSessionOptions::new()
            .with_default_timeout_ms(1)
            .with_file("/tmp/before.txt", "before"),
    );
    let timeout = timed.exec(
        "sleep 50; echo late > /tmp/late.txt",
        JustBashExecOptions::new(),
    );
    assert_eq!(timeout.exit_code, just_bash::JUST_BASH_TIMEOUT_EXIT_CODE);
    assert!(!timed.file_exists("/tmp/late.txt"));
    assert_eq!(timed.read_file("/tmp/before.txt").unwrap(), "before");
}

#[test]
fn just_bash_security_jbc27_policy_and_violation_rows_are_deterministic() {
    let mut log = SecurityViolationLog::with_max_per_code(2);
    for message in ["first", "second", "third"] {
        log.record(SecurityDiagnostic::error(
            SecurityDiagnosticCode::CommandDenied,
            "command",
            message,
        ));
    }
    log.record(SecurityDiagnostic::warning(
        SecurityDiagnosticCode::SensitiveValueRedacted,
        "env",
        "redacted",
    ));

    assert_eq!(log.len(), 3);
    assert_eq!(
        log.counts_by_code()[&SecurityDiagnosticCode::CommandDenied],
        2
    );
    let messages = log
        .entries_most_recent_first()
        .into_iter()
        .map(|entry| entry.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(messages, ["redacted", "third", "second"]);
    log.clear();
    assert!(log.is_empty());
}

#[test]
fn just_bash_security_resource_limits_match_upstream_limit_diagnostics() {
    let limits = ExecutionLimits {
        max_script_bytes: 10,
        max_command_count: 2,
        max_loop_iterations: 3,
        max_call_depth: 4,
        max_string_bytes: 5,
        max_array_elements: 6,
        max_output_bytes: 7,
        max_network_response_bytes: 8,
        max_timeout_ms: 9,
    };

    let diagnostics = limits.check(ResourceObservation {
        script_bytes: 11,
        command_count: 3,
        loop_iterations: 4,
        call_depth: 5,
        string_bytes: 6,
        array_elements: 7,
        output_bytes: 8,
    });

    assert_eq!(diagnostics.len(), 7);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == SecurityDiagnosticCode::LimitExceeded)
    );
    assert_eq!(
        limits.check_output("012345678").unwrap_err().code,
        SecurityDiagnosticCode::LimitExceeded
    );
}

#[test]
fn just_bash_security_timeout_and_abort_share_cancellation_contract() {
    assert_eq!(CancellationState::Running.diagnostic(), None);

    let timed_out = CancellationState::TimedOut {
        elapsed_ms: 125,
        timeout_ms: 100,
    }
    .diagnostic()
    .expect("timeout diagnostic");
    assert_eq!(timed_out.code, SecurityDiagnosticCode::Timeout);
    assert!(timed_out.message.contains("Execution timeout"));

    let cancelled = CancellationState::Cancelled
        .diagnostic()
        .expect("cancel diagnostic");
    assert_eq!(cancelled.code, SecurityDiagnosticCode::Cancelled);
    assert!(CancellationState::Cancelled.is_terminal());
}

#[test]
fn just_bash_network_allow_list_matches_origins_paths_and_bypass_cases() {
    let entries = vec![AllowedUrlEntry::new("https://api.example.com/v1/")];

    assert!(is_url_allowed("https://api.example.com/v1/users", &entries));
    assert!(!is_url_allowed("https://api.example.com/v1", &entries));
    assert!(!is_url_allowed(
        "https://api.example.com/v10/admin",
        &entries
    ));
    assert!(!is_url_allowed(
        "https://api.example.com/v1/%2f..%2fv2/users",
        &entries
    ));
    assert!(!is_url_allowed(
        "https://api.example.com.evil.com/v1/users",
        &entries
    ));
    assert!(!is_url_allowed(
        "https://api.example.com@evil.com/v1/users",
        &entries
    ));
    assert!(!is_url_allowed("http://api.example.com/v1/users", &entries));

    let parsed = Url::parse("https://api.example.com/v1/users").expect("url");
    assert!(matches_allow_list_entry(
        &parsed,
        "https://api.example.com/v1"
    ));
}

#[test]
fn just_bash_network_validates_allow_list_and_defaults_to_no_network() {
    assert_eq!(
        validate_allow_list_entry("ftp://example.com")
            .unwrap_err()
            .code,
        SecurityDiagnosticCode::InvalidAllowList
    );
    assert_eq!(
        validate_allow_list_entry("https://example.com/v1/%2fadmin")
            .unwrap_err()
            .code,
        SecurityDiagnosticCode::InvalidAllowList
    );

    let request = NetworkRequest::new("https://api.example.com/data");
    let diagnostic = plan_network_request(&NetworkPolicy::default(), request, &public_dns)
        .expect_err("network disabled");
    assert_eq!(diagnostic.code, SecurityDiagnosticCode::NetworkDisabled);
}

#[test]
fn just_bash_network_plans_allowed_request_with_fake_dns_and_header_firewall() {
    let policy = NetworkPolicy {
        allowed_url_prefixes: vec![
            AllowedUrlEntry::new("https://api.example.com")
                .with_header("Authorization", "Bearer real-secret"),
        ],
        deny_private_ranges: true,
        ..NetworkPolicy::default()
    };
    let request = NetworkRequest::new("https://api.example.com/data")
        .with_header("authorization", "Bearer user-injected")
        .with_timeout_ms(5_000);

    let planned = plan_network_request(&policy, request, &public_dns).expect("planned request");
    assert_eq!(planned.method, HttpMethod::Get);
    assert_eq!(planned.headers["authorization"], "Bearer real-secret");
    assert_eq!(planned.timeout_ms, 5_000);
    assert_eq!(
        planned.pinned_address,
        Some(DnsAddress::new("93.184.216.34", 4))
    );
}

#[test]
fn just_bash_network_blocks_private_hosts_and_dns_rebinding_before_transport() {
    let policy = NetworkPolicy::full_internet_for_tests().with_private_range_deny(true);
    let resolver_calls = RefCell::new(Vec::<String>::new());
    let resolver = |host: &str| {
        resolver_calls.borrow_mut().push(host.to_string());
        Ok(vec![DnsAddress::new("127.0.0.1", 4)])
    };

    let literal = plan_network_request(
        &policy,
        NetworkRequest::new("https://127.0.0.1/data"),
        &resolver,
    )
    .unwrap_err();
    assert_eq!(literal.code, SecurityDiagnosticCode::PrivateAddressBlocked);
    assert!(resolver_calls.borrow().is_empty());

    let rebound = plan_network_request(
        &policy,
        NetworkRequest::new("https://attacker.example/data"),
        &resolver,
    )
    .unwrap_err();
    assert_eq!(rebound.code, SecurityDiagnosticCode::PrivateAddressBlocked);
    assert_eq!(resolver_calls.borrow().as_slice(), ["attacker.example"]);

    assert!(is_private_hostname("0177.0.0.1"));
    assert!(is_private_hostname("0x7f000001"));
    assert!(is_private_hostname("[::1]"));
}

#[test]
fn just_bash_network_fails_closed_for_dns_errors_but_allows_enotfound_to_fetch() {
    let policy = NetworkPolicy::full_internet_for_tests().with_private_range_deny(true);
    let timeout_resolver = |_host: &str| Err(DnsLookupError::new("ETIMEOUT", "simulated timeout"));
    let diagnostic = plan_network_request(
        &policy,
        NetworkRequest::new("https://timeout.example/data"),
        &timeout_resolver,
    )
    .unwrap_err();
    assert_eq!(diagnostic.code, SecurityDiagnosticCode::DnsResolutionFailed);

    let not_found_resolver = |_host: &str| Err(DnsLookupError::new("ENOTFOUND", "missing domain"));
    let planned = plan_network_request(
        &policy,
        NetworkRequest::new("https://missing.example/data"),
        &not_found_resolver,
    )
    .expect("ENOTFOUND should let fetch fail naturally");
    assert_eq!(planned.pinned_address, None);
}

#[test]
fn just_bash_network_revalidates_redirects_and_repins_each_hop() {
    let policy = NetworkPolicy {
        allowed_url_prefixes: vec![
            AllowedUrlEntry::new("https://first.example"),
            AllowedUrlEntry::new("https://second.example"),
        ],
        deny_private_ranges: true,
        ..NetworkPolicy::default()
    };
    let mut transport = StaticNetworkTransport::new()
        .with_response(
            NetworkResponse::new(302, "https://first.example/start", "")
                .with_header("Location", "https://second.example/landing"),
        )
        .with_response(NetworkResponse::new(
            200,
            "https://second.example/landing",
            "ok",
        ));

    let response = execute_network_request(
        &policy,
        NetworkRequest::new("https://first.example/start"),
        &public_dns,
        &mut transport,
    )
    .expect("redirect request");

    assert_eq!(response.body, b"ok");
    assert_eq!(transport.calls().len(), 2);
    assert_eq!(
        transport.calls()[0].pinned_address,
        Some(DnsAddress::new("8.8.8.8", 4))
    );
    assert_eq!(
        transport.calls()[1].pinned_address,
        Some(DnsAddress::new("1.1.1.1", 4))
    );
}

#[test]
fn just_bash_network_blocks_disallowed_redirect_without_second_transport_call() {
    let policy = NetworkPolicy {
        allowed_url_prefixes: vec![AllowedUrlEntry::new("https://first.example")],
        ..NetworkPolicy::default()
    };
    let mut transport = StaticNetworkTransport::new().with_response(
        NetworkResponse::new(302, "https://first.example/start", "")
            .with_header("Location", "https://evil.example/landing"),
    );

    let diagnostic = execute_network_request(
        &policy,
        NetworkRequest::new("https://first.example/start"),
        &public_dns,
        &mut transport,
    )
    .unwrap_err();

    assert_eq!(diagnostic.code, SecurityDiagnosticCode::NetworkDenied);
    assert_eq!(transport.calls().len(), 1);
}

#[test]
fn just_bash_network_enforces_method_timeout_and_response_limits() {
    let policy = NetworkPolicy::allow_url_prefix("https://api.example.com")
        .with_timeout_ms(1_000)
        .with_max_response_bytes(4);

    let method_error = plan_network_request(
        &policy,
        NetworkRequest::new("https://api.example.com/data").with_method(HttpMethod::Post),
        &public_dns,
    )
    .unwrap_err();
    assert_eq!(method_error.code, SecurityDiagnosticCode::MethodNotAllowed);

    let planned = plan_network_request(
        &policy,
        NetworkRequest::new("https://api.example.com/data").with_timeout_ms(5_000),
        &public_dns,
    )
    .expect("planned");
    assert_eq!(planned.timeout_ms, 1_000);

    let mut transport = StaticNetworkTransport::new().with_response(
        NetworkResponse::new(200, "https://api.example.com/data", "too-large")
            .with_header("content-length", "9"),
    );
    let diagnostic = execute_network_request(
        &policy,
        NetworkRequest::new("https://api.example.com/data"),
        &public_dns,
        &mut transport,
    )
    .unwrap_err();
    assert_eq!(diagnostic.code, SecurityDiagnosticCode::ResponseTooLarge);
}

#[test]
fn just_bash_optional_runtime_security_cases_are_classified_nonportable() {
    let core = classify_runtime_surface(UpstreamRuntimeSurface::CoreSecurity);
    assert!(core.portable_to_rust_backend);

    for surface in [
        UpstreamRuntimeSurface::BrowserBundle,
        UpstreamRuntimeSurface::NodeWorker,
        UpstreamRuntimeSurface::QuickJs,
        UpstreamRuntimeSurface::PythonWasm,
        UpstreamRuntimeSurface::SqliteWasm,
        UpstreamRuntimeSurface::WasmCallback,
    ] {
        let classification = classify_runtime_surface(surface);
        assert!(!classification.portable_to_rust_backend);
        assert!(!classification.reason.is_empty());
    }
}

#[test]
fn just_bash_network_fake_transport_records_only_planned_requests() {
    let policy = NetworkPolicy::allow_url_prefix("https://api.example.com");
    let mut transport = StaticNetworkTransport::new().with_response(NetworkResponse::new(
        200,
        "https://api.example.com/data",
        "ok",
    ));
    let request = NetworkRequest::new("https://api.example.com/data")
        .with_header("X-Trace", "trace-1")
        .with_body("body that GET must drop");

    let response =
        execute_network_request(&policy, request, &public_dns, &mut transport).expect("response");

    assert_eq!(response.body, b"ok");
    assert_eq!(transport.calls().len(), 1);
    assert_eq!(
        transport.calls()[0].headers,
        BTreeMap::from([("x-trace".to_string(), "trace-1".to_string())])
    );
    assert_eq!(transport.calls()[0].body, None);
}
