use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::config::OpenAgentsServiceConfig;

/// Serializable health view exposed at `/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub live: bool,
    pub ready: bool,
    pub state_store: String,
    pub slack_ingress: String,
    pub sandbox: String,
    pub runtime: String,
    pub model: String,
}

/// Mutable health state shared by the service and probe server.
#[derive(Debug, Clone)]
pub struct HealthCheck {
    snapshot: Arc<RwLock<HealthSnapshot>>,
}

impl HealthCheck {
    /// Build a health state from validated service configuration.
    pub fn from_config(config: &OpenAgentsServiceConfig) -> Self {
        Self::new(HealthSnapshot {
            live: true,
            ready: false,
            state_store: config.state_store().label().to_string(),
            slack_ingress: format!("{:?}", config.slack_ingress()),
            sandbox: config.sandbox().label().to_string(),
            runtime: config.runtime().label().to_string(),
            model: config.model_id().to_string(),
        })
    }

    /// Build a health state from a caller-provided snapshot.
    pub fn new(snapshot: HealthSnapshot) -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(snapshot)),
        }
    }

    /// Return the current snapshot.
    pub fn snapshot(&self) -> HealthSnapshot {
        self.snapshot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Mark readiness for `/readyz`.
    pub fn set_ready(&self, ready: bool) {
        self.snapshot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .ready = ready;
    }

    pub(crate) fn response_for_path(&self, path: &str) -> (u16, &'static str, String) {
        let snapshot = self.snapshot();
        match path {
            "/healthz" => {
                if snapshot.live {
                    (200, "text/plain; charset=utf-8", "ok\n".to_string())
                } else {
                    (503, "text/plain; charset=utf-8", "not live\n".to_string())
                }
            }
            "/readyz" => {
                if snapshot.ready {
                    (200, "text/plain; charset=utf-8", "ok\n".to_string())
                } else {
                    (503, "text/plain; charset=utf-8", "not ready\n".to_string())
                }
            }
            "/status" => (
                200,
                "application/json",
                serde_json::to_string(&snapshot).expect("HealthSnapshot should serialize to JSON"),
            ),
            _ => (404, "text/plain; charset=utf-8", "not found\n".to_string()),
        }
    }
}

/// Health server errors.
#[derive(Debug)]
pub enum HealthError {
    Io(std::io::Error),
}

impl fmt::Display for HealthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "health server I/O error: {err}"),
        }
    }
}

impl std::error::Error for HealthError {}

impl From<std::io::Error> for HealthError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Bind the health listener. Split out so tests can bind port 0 and inspect it.
pub async fn bind_health_listener(addr: SocketAddr) -> Result<TcpListener, HealthError> {
    TcpListener::bind(addr).await.map_err(Into::into)
}

/// Serve health probes until `shutdown` resolves.
pub async fn serve_health_checks(
    listener: TcpListener,
    health: HealthCheck,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), HealthError> {
    let mut shutdown = Box::pin(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let health = health.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(stream, health).await;
                });
            }
        }
    }
}

async fn handle_connection(mut stream: TcpStream, health: HealthCheck) -> Result<(), HealthError> {
    let mut buffer = [0_u8; 4096];
    let read = stream.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    let (status, content_type, body) = health.response_for_path(path);
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenAgentsServiceConfig;

    async fn get(addr: SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nhost: localhost\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn healthz_and_readyz_reflect_liveness_and_readiness() {
        let config = OpenAgentsServiceConfig::fixture();
        let health = HealthCheck::from_config(&config);
        let listener = bind_health_listener(config.bind_addr()).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_health_checks(listener, health.clone(), async move {
            let _ = stop_rx.await;
        }));

        let response = get(addr, "/healthz").await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("ok\n"));

        let response = get(addr, "/readyz").await;
        assert!(response.starts_with("HTTP/1.1 503 Service Unavailable"));

        health.set_ready(true);
        let response = get(addr, "/readyz").await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));

        stop_tx.send(()).unwrap();
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn status_returns_json_snapshot() {
        let config = OpenAgentsServiceConfig::fixture();
        let health = HealthCheck::from_config(&config);
        health.set_ready(true);
        let listener = bind_health_listener(config.bind_addr()).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_health_checks(listener, health, async move {
            let _ = stop_rx.await;
        }));

        let response = get(addr, "/status").await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("\"ready\":true"));
        assert!(response.contains("\"state_store\":\"memory\""));

        stop_tx.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
}
