use http_body_util::BodyExt as _;
use vercel_runtime::{Error, Request, Response, ResponseBody, run, service_fn};

use crate::{OpenAgentsService, OpenAgentsServiceConfig, ServiceHttpRequest};

/// Run a Vercel Rust Runtime function for a single Open Agents service route.
pub async fn run_service_route(route_path: &'static str) -> Result<(), Error> {
    run(service_fn(move |request| {
        handle_request(request, route_path)
    }))
    .await
}

async fn handle_request(
    request: Request,
    route_path: &'static str,
) -> Result<Response<ResponseBody>, Error> {
    let (parts, body) = request.into_parts();
    let method = parts.method.as_str().to_string();
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    let body = match body.collect().await {
        Ok(collected) => match String::from_utf8(collected.to_bytes().to_vec()) {
            Ok(body) => body,
            Err(_) => return text_response(400, "request body is not UTF-8\n"),
        },
        Err(error) => return text_response(400, format!("failed to read request body: {error}\n")),
    };
    let config = match OpenAgentsServiceConfig::from_env() {
        Ok(config) => config,
        Err(error) => return text_response(500, format!("{error}\n")),
    };
    let service = match OpenAgentsService::from_config(config) {
        Ok(service) => service,
        Err(error) => return text_response(500, format!("{error}\n")),
    };
    service.health().set_ready(true);

    let response = service
        .handle_http_request(ServiceHttpRequest::new(method, route_path, headers, body))
        .await;
    Response::builder()
        .status(response.status)
        .header("content-type", response.content_type)
        .body(ResponseBody::from(response.body))
        .map_err(Into::into)
}

fn text_response(status: u16, body: impl Into<String>) -> Result<Response<ResponseBody>, Error> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(ResponseBody::from(body.into()))
        .map_err(Into::into)
}
