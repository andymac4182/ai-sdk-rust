use std::collections::BTreeMap;

use just_bash::{
    Bash, BashOptions, DnsAddress, HttpMethod, NetworkPolicy, NetworkRequest, NetworkResponse,
    StaticNetworkTransport, execute_network_request,
};

fn project_bash() -> Bash {
    Bash::with_options(BashOptions {
        cwd: Some("/project".to_string()),
        files: BTreeMap::from([
            ("/project/README.md".to_string(), "# Project".to_string()),
            (
                "/project/src/index.ts".to_string(),
                "export {}\n".to_string(),
            ),
            (
                "/project/src/utils/format.ts".to_string(),
                "export function format() {}\n".to_string(),
            ),
            (
                "/project/src/utils/helpers.ts".to_string(),
                "export function helper() {}\n".to_string(),
            ),
            (
                "/project/tests/index.test.ts".to_string(),
                "test('works', () => {})\n".to_string(),
            ),
            ("/project/package.json".to_string(), "{}".to_string()),
            ("/project/empty.txt".to_string(), String::new()),
        ]),
        ..BashOptions::default()
    })
}

#[test]
fn jbc22_find_command_closes_basic_pattern_depth_operator_action_rows() {
    let env = project_bash();

    assert_eq!(
        env.exec("find /project -type f -name '*.ts'").stdout,
        "/project/src/index.ts\n/project/src/utils/format.ts\n/project/src/utils/helpers.ts\n/project/tests/index.test.ts\n"
    );
    assert_eq!(env.exec("find . -name '*.md'").stdout, "./README.md\n");
    assert_eq!(
        env.exec("find /project -maxdepth 1 -type f").stdout,
        "/project/README.md\n/project/empty.txt\n/project/package.json\n"
    );
    assert_eq!(
        env.exec("find /project -name '?????.json' -o -iname 'readme.md'")
            .stdout,
        "/project/README.md\n"
    );
    let missing = env.exec("find /missing /project -type d");
    assert_eq!(missing.exit_code, 1);
    assert!(
        missing
            .stderr
            .contains("find: /missing: No such file or directory")
    );
    assert!(missing.stdout.contains("/project/src\n"));
}

#[test]
fn jbc22_find_command_closes_printf_delete_exec_and_metadata_rows() {
    let env = project_bash();

    assert_eq!(
        env.exec("find /project -type f -size 2c -printf '%P:%s:%m\\n'")
            .stdout,
        "package.json:2:644\n"
    );
    assert_eq!(
        env.exec("find /project -type f -empty -print0").stdout,
        "/project/empty.txt\0"
    );
    assert_eq!(
        env.exec("find /project/src -type f -name '*.ts' -exec echo files:{} +")
            .stdout,
        "files:/project/src/index.ts /project/src/utils/format.ts /project/src/utils/helpers.ts\n"
    );
    let delete = env.exec("find /project -name 'empty.txt' -delete");
    assert_eq!(delete.exit_code, 0);
    assert_eq!(env.exec("find /project -name 'empty.txt'").stdout, "");
}

fn curl_bash(policy: NetworkPolicy, responses: Vec<NetworkResponse>) -> Bash {
    Bash::with_options(BashOptions {
        network_policy: Some(policy),
        network_responses: responses
            .into_iter()
            .map(|response| (response.url.clone(), response))
            .collect(),
        files: BTreeMap::from([
            ("/payload.txt".to_string(), "alpha\r\nbeta\n".to_string()),
            ("/extra.txt".to_string(), "a=b & c".to_string()),
        ]),
        ..BashOptions::default()
    })
}

#[test]
fn jbc22_curl_command_uses_opt_in_fake_transport_and_resource_policy() {
    let default_env = Bash::new();
    let default = default_env.exec("curl https://api.example.com/data");
    assert_eq!(default.exit_code, 127);
    assert!(default.stderr.contains("command not found"));

    let policy = NetworkPolicy::allow_url_prefix("https://api.example.com")
        .with_allowed_method(HttpMethod::Post)
        .with_allowed_method(HttpMethod::Put);
    let env = curl_bash(
        policy,
        vec![
            NetworkResponse::new(200, "https://api.example.com/data", "hello")
                .with_header("content-type", "text/plain"),
            NetworkResponse::new(201, "https://api.example.com/post", "created")
                .with_header("set-cookie", "sid=abc"),
        ],
    );

    assert_eq!(
        env.exec("curl https://api.example.com/data").stdout,
        "hello"
    );
    let denied = env.exec("curl https://evil.example.com/data");
    assert_eq!(denied.exit_code, 1);
    assert!(denied.stderr.contains("Network access denied"));
}

#[test]
fn jbc22_curl_command_closes_parse_body_output_cookie_and_error_rows() {
    let policy = NetworkPolicy::allow_url_prefix("https://api.example.com")
        .with_allowed_method(HttpMethod::Post)
        .with_allowed_method(HttpMethod::Put);
    let env = curl_bash(
        policy,
        vec![
            NetworkResponse::new(200, "https://api.example.com/post", "ok")
                .with_header("content-type", "application/json")
                .with_header("set-cookie", "sid=abc"),
            NetworkResponse::new(404, "https://api.example.com/missing", "missing"),
            NetworkResponse::new(200, "https://api.example.com/file.bin", vec![0, 1, 255]),
        ],
    );

    let posted = env.exec(
        "curl -sS -d @/payload.txt --data-urlencode extra@/extra.txt -c /cookies.txt -w '\\n%{http_code} %{content_type} %{size_download}' https://api.example.com/post",
    );
    assert_eq!(posted.exit_code, 0);
    assert_eq!(posted.stdout, "ok\n200 application/json 2");
    assert_eq!(env.read_file("/cookies.txt").unwrap(), "sid=abc");

    let failed = env.exec("curl -f -sS https://api.example.com/missing");
    assert_eq!(failed.exit_code, 22);
    assert!(failed.stderr.contains("404"));

    let output = env.exec("curl -o /download.bin https://api.example.com/file.bin");
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        env.read_file_buffer("/download.bin").unwrap(),
        vec![0, 1, 255]
    );
}

#[test]
fn jbc22_network_resource_seam_records_planned_fake_requests_without_live_io() {
    let policy = NetworkPolicy::allow_url_prefix("https://api.example.com")
        .with_allowed_method(HttpMethod::Post)
        .with_private_range_deny(true);
    let request = NetworkRequest::new("https://api.example.com/resource")
        .with_method(HttpMethod::Post)
        .with_header("X-Test", "one")
        .with_body(b"payload".to_vec());
    let resolver = |_host: &str| Ok(vec![DnsAddress::new("93.184.216.34", 4)]);
    let mut transport = StaticNetworkTransport::new().with_response(NetworkResponse::new(
        200,
        "https://api.example.com/resource",
        "resource",
    ));

    let response = execute_network_request(&policy, request, &resolver, &mut transport).unwrap();
    assert_eq!(response.body, b"resource");
    assert_eq!(transport.calls().len(), 1);
    assert_eq!(transport.calls()[0].method, HttpMethod::Post);
    assert_eq!(transport.calls()[0].body.as_deref(), Some(&b"payload"[..]));
    assert_eq!(
        transport.calls()[0]
            .headers
            .get("x-test")
            .map(String::as_str),
        Some("one")
    );
}
