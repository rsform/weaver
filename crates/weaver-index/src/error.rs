use miette::Diagnostic;
use thiserror::Error;

/// Top-level error type for weaver-index operations
#[derive(Debug, Error, Diagnostic)]
pub enum IndexError {
    #[error(transparent)]
    #[diagnostic(transparent)]
    ClickHouse(#[from] ClickHouseError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Firehose(#[from] FirehoseError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Car(#[from] CarError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    Config(#[from] ConfigError),
}

/// ClickHouse database errors
#[derive(Debug, Error, Diagnostic)]
pub enum ClickHouseError {
    #[error("failed to connect to ClickHouse: {message}")]
    #[diagnostic(code(clickhouse::connection))]
    Connection {
        message: String,
        #[source]
        source: clickhouse::error::Error,
    },

    #[error("ClickHouse query failed: {message}")]
    #[diagnostic(code(clickhouse::query))]
    Query {
        message: String,
        #[source]
        source: clickhouse::error::Error,
    },

    #[error("failed to insert batch: {message}")]
    #[diagnostic(code(clickhouse::insert))]
    Insert {
        message: String,
        #[source]
        source: clickhouse::error::Error,
    },

    #[error("schema migration failed: {message}")]
    #[diagnostic(code(clickhouse::schema))]
    Schema { message: String },
}

/// Firehose/subscription stream errors
#[derive(Debug, Error, Diagnostic)]
pub enum FirehoseError {
    #[error("failed to connect to relay at {url}")]
    #[diagnostic(code(firehose::connection))]
    Connection { url: String, message: String },

    #[error("websocket stream error: {message}")]
    #[diagnostic(code(firehose::stream))]
    Stream { message: String },

    #[error("failed to parse event header")]
    #[diagnostic(code(firehose::header))]
    HeaderParse {
        #[source]
        source: ciborium::de::Error<std::io::Error>,
    },

    #[error("failed to decode event body: {event_type}")]
    #[diagnostic(code(firehose::decode))]
    BodyDecode { event_type: String, message: String },

    #[error("unknown event type: {event_type}")]
    #[diagnostic(code(firehose::unknown_event))]
    UnknownEvent { event_type: String },
}

/// CAR file parsing errors
#[derive(Debug, Error, Diagnostic)]
pub enum CarError {
    #[error("failed to parse CAR data")]
    #[diagnostic(code(car::parse))]
    Parse { message: String },

    #[error("block not found for CID: {cid}")]
    #[diagnostic(code(car::block_not_found))]
    BlockNotFound { cid: String },

    #[error("failed to decode record from block: {message}")]
    #[diagnostic(code(car::record_decode))]
    RecordDecode { message: String },
}

/// Configuration errors
#[derive(Debug, Error, Diagnostic)]
pub enum ConfigError {
    #[error("missing required environment variable: {var}")]
    #[diagnostic(
        code(config::missing_env),
        help("Set the {var} environment variable or add it to your .env file")
    )]
    MissingEnv { var: &'static str },

    #[error("invalid configuration value for {field}: {message}")]
    #[diagnostic(code(config::invalid))]
    Invalid { field: &'static str, message: String },

    #[error("failed to parse URL: {url}")]
    #[diagnostic(code(config::url_parse))]
    UrlParse { url: String, message: String },
}

pub type Result<T> = std::result::Result<T, IndexError>;
