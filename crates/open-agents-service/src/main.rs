#![forbid(unsafe_code)]

fn main() {
    let _required_env = open_agents_service::required_slack_env();
}
