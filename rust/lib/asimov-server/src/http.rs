// This is free and unencumbered software released into the public domain.

mod graphql;
mod gsp;
pub mod mcp;
mod openai;
mod openai_v1;
mod prometheus;
mod sparql;
mod well_known;

#[cfg(feature = "app")]
mod app;

use axum::{Router, response::Json, routing::get};
use tokio::net::{TcpListener, ToSocketAddrs};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;

pub fn routes() -> Router {
    let mcp_server = mcp::Server::default();
    let router = Router::new()
        .merge(graphql::routes())
        .merge(gsp::routes())
        .merge(mcp::routes().with_state(mcp_server))
        .merge(openai::routes())
        .merge(prometheus::routes())
        .merge(sparql::routes())
        .merge(well_known::routes());

    #[cfg(feature = "app")]
    let router = router.merge(app::routes());

    #[cfg(feature = "tracing")]
    let router = router.layer(
        tower_http::trace::TraceLayer::new_for_http()
            // Inbound headers can contain credentials even when not marked sensitive.
            .make_span_with(tower_http::trace::DefaultMakeSpan::new().include_headers(false))
            .on_request(
                |request: &http::Request<axum::body::Body>, _span: &tracing::Span| {
                    tracing::info!(
                        "Received a {} {} request",
                        request.method(),
                        request.uri().path()
                    );
                },
            ),
    );

    router
        .layer(CorsLayer::permissive())
        .route("/", get(http_handler))
}

pub async fn start(addr: impl ToSocketAddrs, cancel: CancellationToken) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;

    #[cfg(feature = "tracing")]
    tracing::info!(
        "Listening for HTTP requests on {}...",
        listener.local_addr().unwrap()
    );

    axum::serve(listener, routes())
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
}

async fn http_handler() -> Json<&'static str> {
    Json("Hello, world!") // TODO
}

#[cfg(all(test, feature = "tracing"))]
mod tests {
    use super::*;
    use alloc::{string::String, sync::Arc};
    use core::fmt::{Debug, Write as _};
    use std::sync::Mutex;
    use tracing::{
        Subscriber,
        field::{Field, Visit},
        instrument::WithSubscriber,
    };
    use tracing_subscriber::{Layer, layer::Context, prelude::*};

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<String>>);

    impl Visit for Capture {
        fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
            writeln!(self.0.lock().unwrap(), "{field}={value:?}").unwrap();
        }
    }

    impl<S: Subscriber> Layer<S> for Capture {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _: &tracing::span::Id,
            _: Context<'_, S>,
        ) {
            attrs.record(&mut self.clone());
        }
    }

    #[tokio::test]
    async fn request_spans_omit_unmarked_credentials() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        async {
            let server = axum_test::TestServer::new(routes()).unwrap();
            server
                .post("/graphql")
                .add_header(http::header::AUTHORIZATION, "Bearer private-test-token")
                .add_header(http::header::COOKIE, "session=private-test-cookie")
                .add_header("x-api-key", "private-test-api-key")
                .await
                .assert_status_ok();
        }
        .with_subscriber(subscriber)
        .await;

        let spans = capture.0.lock().unwrap();
        assert!(spans.contains("POST"), "no request span captured: {spans}");
        assert!(spans.contains("/graphql"));
        for secret in [
            "private-test-token",
            "private-test-cookie",
            "private-test-api-key",
        ] {
            assert!(!spans.contains(secret), "request span leaked a credential");
        }
    }
}
