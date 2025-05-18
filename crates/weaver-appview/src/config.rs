use miette::miette;
use serde::{Deserialize, Serialize};
use std::{env, fs};

#[derive(Deserialize, Serialize, Clone)]
pub struct OauthConfig {
    pub jwks: Vec<jose_jwk::Jwk>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct JetstreamConfig {
    pub endpoint: String,
}

impl Default for JetstreamConfig {
    fn default() -> Self {
        Self {
            endpoint: "wss://jetstream1.us-east.bsky.network/subscribe".into(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct CoreConfig {
    pub db_path: String,
    pub listen_addr: String,
    pub appview_host: String,
    pub cookie_secret: String,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            db_path: "postgres://postgres:@localhost/weaver_appview".into(),
            listen_addr: "0.0.0.0:4000".into(),
            appview_host: "https://appview.weaver.sh".into(),
            cookie_secret: "00000000000000000000000000000000".into(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub oauth: OauthConfig,
    pub jetstream: JetstreamConfig,
    pub core: CoreConfig,
}

impl Config {
    pub fn load(config_file: &str) -> miette::Result<Config> {
        let mut config_string = fs::read_to_string(config_file)
            .map_err(|e| miette!("error reading config file {}", e))?;
        // substitute environment variables in config file
        for (k, v) in env::vars() {
            config_string = config_string.replace(&format!("${}", k), &v);
        }

        Ok(toml::from_str(&config_string)
            .map_err(|e| miette!("error parsing config file {}", e))?)
    }
}
