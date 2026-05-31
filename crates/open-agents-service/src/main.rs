#![forbid(unsafe_code)]

use std::error::Error;

use open_agents_service::{
    FixtureHarness, OpenAgentsService, OpenAgentsServiceConfig, bind_service_listener,
    serve_open_agents_service,
};
use serde_json::json;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("open-agents-slack failed: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("--check-config") => {
            let config = OpenAgentsServiceConfig::from_env()?;
            println!("{}", config.operator_summary());
            Ok(())
        }
        Some("--fixture") => run_fixture(),
        Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some(arg) => Err(format!("unknown argument {arg:?}").into()),
        None => run_server().await,
    }
}

async fn run_server() -> Result<(), Box<dyn Error>> {
    let config = OpenAgentsServiceConfig::from_env()?;
    let service = OpenAgentsService::from_config(config.clone())?;
    let listener = bind_service_listener(config.bind_addr()).await?;
    let local_addr = listener.local_addr()?;
    service.health().set_ready(true);
    eprintln!(
        "open-agents-slack listening on http://{local_addr} ({})",
        config.operator_summary()
    );
    serve_open_agents_service(listener, service, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

fn run_fixture() -> Result<(), Box<dyn Error>> {
    let harness = FixtureHarness::new()?;
    let body = json!({
        "type": "event_callback",
        "team_id": "TLOCAL",
        "api_app_id": "ALOCAL",
        "event_id": "EvLOCAL",
        "event_time": 1710000000,
        "event": {
            "type": "app_mention",
            "user": "ULOCAL",
            "text": "inspect the repo",
            "channel": "CLOCAL",
            "ts": "1710000000.000100"
        }
    })
    .to_string();
    harness.handle_slack_body(&body, "application/json")?;
    for message in harness.outbound_messages() {
        println!(
            "{} {} {}",
            message.thread_id,
            message.kind.as_str(),
            message.text
        );
    }
    Ok(())
}

fn print_help() {
    println!("Usage: open-agents-slack [--check-config|--fixture]");
    println!("No arguments starts the HTTP service with health and Slack routes.");
}
