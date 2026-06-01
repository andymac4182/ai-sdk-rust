use std::cell::RefCell;
use std::collections::BTreeMap;

use just_bash::security::ExecutionLimits;
use just_bash::{
    AllowedUrlEntry, CancellationState, CommandSecurityPolicy, DnsAddress, DnsLookupError,
    HttpMethod, NetworkPolicy, NetworkRequest, NetworkResponse, ResourceObservation,
    SecurityDiagnosticCode, StaticNetworkTransport, UpstreamRuntimeSurface,
    classify_runtime_surface, execute_network_request, is_private_hostname, is_url_allowed,
    matches_allow_list_entry, plan_network_request, validate_allow_list_entry,
    validate_workspace_path,
};
use url::Url;

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
