use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_appender::{self, non_blocking, non_blocking::WorkerGuard, rolling::daily};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, layer, writer::MakeWriterExt},
    layer::SubscriberExt,
    registry,
    util::SubscriberInitExt,
};
/// The `EnvFilter` type is used to filter log events based on the value of an environment variable.
/// In this case, we are using the `try_from_default_env` method to attempt to read the `RUST_LOG` environment variable,
/// which is used to set the log level for the application.
/// If the environment variable is not set, we default to the log level of `debug`.
/// The `RUST_LOG` environment variable is set in the Dockerfile and .env files.
pub fn setup_tracing<S: AsRef<str>>(logdir: S) -> WorkerGuard {
    let (non_blocking_appender, guard) = non_blocking(daily(logdir.as_ref(), "general.log"));
    let env_filter_layer = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!(
					"debug,{}=debug,tower_http=debug,axum=debug,hyper=debug,axum::rejection=trace,markdown=info",
					env!("CARGO_PKG_NAME"),
				).into()
    });
    let formatting_layer = fmt::layer().json();
    tracing_subscriber::registry()
        .with(env_filter_layer)
        .with(formatting_layer)
        .with(
            layer()
                .with_writer(std::io::stdout.with_max_level(Level::DEBUG))
                .event_format(tracing_subscriber::fmt::format().pretty()),
        )
        .with(layer().with_writer(non_blocking_appender.with_max_level(Level::INFO)))
        .init();
    guard
}

/// Returns a `TraceLayer` for HTTP requests and responses.
/// The `TraceLayer` is used to trace requests and responses in the application.
pub fn trace_layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>> {
    TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO))
}
