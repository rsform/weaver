pub mod api_error;

pub mod config;
pub mod db;
pub mod middleware;
pub mod models;
pub mod oauth;
pub mod routes;
pub mod schema;
pub mod state;
pub mod telemetry;

use axum::Router;
use clap::Parser;
use config::*;
use db::*;
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use dotenvy::dotenv;
use miette::IntoDiagnostic;
use miette::miette;
use state::*;
use std::env;

use tokio::net::TcpListener;
use tracing::{debug, error, info};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(
        short,
        long,
        value_name = "FILE",
        default_value = "appview-config.toml"
    )]
    config: String,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let config = initialize()?;
    // Run any migrations before we do anything else.
    let db_path = config.core.db_path.clone();
    let _ = tokio::task::spawn_blocking(|| db::run_migrations(Some(db_path)))
        .await
        .into_diagnostic()?;
    let db = Db::new(Some(config.core.db_path.clone())).await;
    debug!("Connected to database");
    // Spin up our server.
    info!("Starting server on {}", config.core.listen_addr);
    let listener = TcpListener::bind(&config.core.listen_addr)
        .await
        .expect("Failed to bind address");
    let router = router(config, db);
    axum::serve(listener, router)
        .await
        .expect("Failed to start server");
    Ok(())
}

pub fn router(cfg: Config, db: Db) -> Router {
    let app_state = AppState::new(cfg, db);

    // Middleware that adds high level tracing to a Service.
    // Trace comes with good defaults but also supports customizing many aspects of the output:
    // https://docs.rs/tower-http/latest/tower_http/trace/index.html
    let trace_layer = telemetry::trace_layer();

    // Sets 'x-request-id' header with randomly generated uuid v7.
    let request_id_layer = middleware::request_id_layer();

    // Propagates 'x-request-id' header from the request to the response.
    let propagate_request_id_layer = middleware::propagate_request_id_layer();

    // Layer that applies the Cors middleware which adds headers for CORS.
    let cors_layer = middleware::cors_layer();

    // Layer that applies the Timeout middleware, which sets a timeout for requests.
    // The default value is 15 seconds.
    let timeout_layer = middleware::timeout_layer();

    // Any trailing slashes from request paths will be removed. For example, a request with `/foo/`
    // will be changed to `/foo` before reaching the internal service.
    let normalize_path_layer = middleware::normalize_path_layer();

    // Create the router with the routes.
    let router = routes::router();

    // Combine all the routes and apply the middleware layers.
    // The order of the layers is important. The first layer is the outermost layer.
    Router::new()
        .merge(router)
        .layer(normalize_path_layer)
        .layer(cors_layer)
        .layer(timeout_layer)
        .layer(propagate_request_id_layer)
        .layer(trace_layer)
        .layer(request_id_layer)
        .with_state(app_state)
}

pub fn initialize() -> miette::Result<Config> {
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                //.rgb_colors(miette::RgbColors::)
                .with_cause_chain()
                .with_syntax_highlighting(miette::highlighters::SyntectHighlighter::default())
                .color(true)
                .context_lines(5)
                .tab_width(2)
                .break_words(true)
                .build(),
        )
    }))
    .map_err(|e| miette!("Failed to set miette hook: {}", e))?;
    miette::set_panic_hook();
    dotenv().ok();
    let cli = Cli::parse();
    let config = config::Config::load(&cli.config);
    let config = if let Err(e) = config {
        error!("{}", e);
        config::Config::load(
            &env::var("APPVIEW_CONFIG").expect("Either set APPVIEW_CONFIG to the path to your config file, pass --config FILE to specify the path, or create a file called appview-config.toml in the directory where you are running the binary from."),
        )
        .map_err(|e| miette!(e))
    } else {
        config
    }?;
    let log_dir = env::var("LOG_DIR").unwrap_or_else(|_| "/tmp/appview".to_string());
    std::fs::create_dir_all(&log_dir).unwrap();
    let _guard = telemetry::setup_tracing(&log_dir);
    Ok(config)
}
