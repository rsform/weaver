use atrium_api::agent::atp_agent::AtpSession;
use miette::Result;
use miette::miette;
use serde::{Deserialize, Serialize};

use std::future::Future;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// The base URL for the XRPC endpoint.
    pub endpoint: String,
    /// The session data.
    pub session: Option<AtpSession>,
    /// The labelers header values.
    pub labelers_header: Option<Vec<String>>,
    /// The proxy header for service proxying.
    pub proxy_header: Option<String>,
}

impl Config {
    /// Loads the configuration from the provided loader.
    pub async fn load(loader: &impl Loader) -> Result<Self> {
        loader
            .load()
            .await
            .map_err(|_| miette!("Failed to load configuration"))
    }
    /// Saves the configuration using the provided saver.
    pub async fn save(&self, saver: &impl Saver) -> Result<()> {
        saver
            .save(self)
            .await
            .map_err(|_| miette!("Failed to save configuration"))
    }
}

impl Default for Config {
    /// Creates a new default configuration.
    ///
    /// The default configuration uses the base URL `https://bsky.social`.
    fn default() -> Self {
        Self {
            endpoint: "https://atproto.systems".to_owned(),
            session: None,
            labelers_header: None,
            proxy_header: None,
        }
    }
}

/// The trait for loading configuration data.
pub trait Loader {
    /// Loads the configuration data.
    fn load(
        &self,
    ) -> impl Future<
        Output = core::result::Result<Config, Box<dyn std::error::Error + Send + Sync + 'static>>,
    > + Send;
}

/// The trait for saving configuration data.
pub trait Saver {
    /// Saves the configuration data.
    fn save(
        &self,
        config: &Config,
    ) -> impl Future<
        Output = core::result::Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>,
    > + Send;
}

/// An implementation of [`Loader`] and [`Saver`] that reads and writes a configuration file.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// Create a new [`FileStore`] with the given path.
    ///
    /// This `FileStore` will read and write to the file at the given path.
    /// [`Config`] data will be serialized and deserialized using the file extension.
    /// By default, this supports only `.json` files.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl Loader for FileStore {
    async fn load(
        &self,
    ) -> core::result::Result<Config, Box<dyn std::error::Error + Send + Sync + 'static>> {
        match self.path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Ok(serde_json::from_str(&std::fs::read_to_string(&self.path)?)?),
            Some("toml") => Ok(toml::from_str(&std::fs::read_to_string(&self.path)?)?),
            _ => Err(miette!("Unsupported file format").into()),
        }
    }
}

impl Saver for FileStore {
    async fn save(
        &self,
        config: &Config,
    ) -> core::result::Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        match self.path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Ok(std::fs::write(
                &self.path,
                serde_json::to_string_pretty(config)?,
            )?),
            Some("toml") => Ok(std::fs::write(&self.path, toml::to_string_pretty(config)?)?),
            _ => Err(miette!("Unsupported file format").into()),
        }
    }
}
