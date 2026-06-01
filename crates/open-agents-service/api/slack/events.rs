#![forbid(unsafe_code)]

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    open_agents_service::vercel_adapter::run_service_route("/slack/events").await
}
