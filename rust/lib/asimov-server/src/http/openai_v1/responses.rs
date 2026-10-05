// This is free and unencumbered software released into the public domain.

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{delete, get, post},
};
use serde_json::{Value, json};

/// See: <https://platform.openai.com/docs/api-reference/responses>
pub fn routes() -> Router {
    Router::new()
        .route("/", post(unsupported))
        .route("/{response_id}", get(unsupported))
        .route("/{response_id}", delete(unsupported))
        .route("/{response_id}/input_items", get(unsupported))
}

async fn unsupported() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "The Responses API is not implemented.",
                "type": "server_error",
                "param": null,
                "code": "not_implemented"
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_mounted_response_endpoint_returns_an_error_envelope() {
        let server = axum_test::TestServer::new(crate::http::openai::routes()).unwrap();
        for response in [
            server
                .post("/v1/responses")
                .json(&json!({"model": "example", "input": "Hello"}))
                .await,
            server.get("/v1/responses/resp_example").await,
            server.delete("/v1/responses/resp_example").await,
            server.get("/v1/responses/resp_example/input_items").await,
        ] {
            response.assert_status(StatusCode::NOT_IMPLEMENTED);
            response.assert_json(&json!({
                "error": {
                    "message": "The Responses API is not implemented.",
                    "type": "server_error",
                    "param": null,
                    "code": "not_implemented"
                }
            }));
        }
    }
}
