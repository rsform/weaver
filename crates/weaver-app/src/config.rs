use core::fmt;
use std::str::FromStr;

use jacquard::{
    CowStr, IntoStatic,
    oauth::{
        atproto::{AtprotoClientMetadata, GrantType},
        scopes::Scope,
    },
    smol_str::{SmolStr, ToSmolStr},
    url::Url,
};

use crate::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub oauth: AtprotoClientMetadata<'static>,
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: jacquard::url::Url,
    pub redirect_uri: jacquard::url::Url,
    pub scopes: Vec<Scope<'static>>,
    pub client_name: SmolStr,
    pub client_uri: Option<jacquard::url::Url>,
    pub logo_uri: Option<jacquard::url::Url>,
    pub tos_uri: Option<jacquard::url::Url>,
    pub privacy_policy_uri: Option<jacquard::url::Url>,
}

impl OAuthConfig {
    /// This will panic if something is incorrect. You kind of can't proceed if these aren't a certain way, so...
    pub fn new(
        client_id: jacquard::url::Url,
        redirect_uri: jacquard::url::Url,
        scopes: Vec<Scope<'static>>,
        client_name: SmolStr,
        client_uri: Option<jacquard::url::Url>,
        logo_uri: Option<jacquard::url::Url>,
        tos_uri: Option<jacquard::url::Url>,
        privacy_policy_uri: Option<jacquard::url::Url>,
    ) -> Self {
        let scopes = if scopes.is_empty() {
            vec![
                Scope::Atproto,
                Scope::Transition(jacquard::oauth::scopes::TransitionScope::Generic),
            ]
        } else {
            scopes
        };
        if let Some(client_uri) = &client_uri {
            if let Some(client_uri_host) = client_uri.host_str() {
                if client_uri_host != client_id.host_str().expect("client_id must have a host") {
                    panic!("client_uri host must match client_id host");
                }
            }
        }
        if let Some(logo_uri) = &logo_uri {
            if logo_uri.scheme() != "https" {
                panic!("logo_uri scheme must be https");
            }
        }
        if let Some(tos_uri) = &tos_uri {
            if tos_uri.scheme() != "https" {
                panic!("tos_uri scheme must be https");
            }
        }
        if let Some(privacy_policy_uri) = &privacy_policy_uri {
            if privacy_policy_uri.scheme() != "https" {
                panic!("privacy_policy_uri scheme must be https");
            }
        }
        Self {
            client_id,
            redirect_uri,
            scopes,
            client_name,
            client_uri,
            logo_uri,
            tos_uri,
            privacy_policy_uri,
        }
    }

    pub fn new_dev(port: u32, scopes: Vec<Scope<'static>>, client_name: SmolStr) -> Self {
        // determine client_id
        #[derive(serde::Serialize)]
        struct Parameters<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            redirect_uri: Option<Vec<Url>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            scope: Option<CowStr<'a>>,
        }
        let redirect_uri: Url = format!("http://127.0.0.1:{port}/callback").parse().unwrap();
        let query = serde_html_form::to_string(Parameters {
            redirect_uri: Some(vec![redirect_uri.clone()]),
            scope: Some(Scope::serialize_multiple(scopes.as_slice())),
        })
        .ok();
        let mut client_id = String::from("http://localhost");
        if let Some(query) = query
            && !query.is_empty()
        {
            client_id.push_str(&format!("?{query}"));
        };
        Self::new(
            client_id.parse().unwrap(),
            redirect_uri,
            scopes,
            client_name,
            None,
            None,
            None,
            None,
        )
    }

    pub fn from_env() -> Self {
        let app_env = AppEnv::from_str(env::WEAVER_APP_ENV).unwrap_or(AppEnv::Dev);

        if app_env == AppEnv::Dev {
            Self::new_dev(
                env::WEAVER_PORT.parse().unwrap_or(8080),
                Scope::parse_multiple(env::WEAVER_APP_SCOPES)
                    .unwrap_or(vec![])
                    .into_static(),
                env::WEAVER_CLIENT_NAME.to_smolstr(),
            )
        } else {
            let host = env::WEAVER_APP_HOST;
            let client_id = format!("{host}/oauth-client-metadata.json");
            let redirect_uri = format!("{host}/callback");
            let logo_uri = if env::WEAVER_LOGO_URI.is_empty() {
                None
            } else {
                Url::parse(env::WEAVER_LOGO_URI).ok()
            };
            let tos_uri = if env::WEAVER_TOS_URI.is_empty() {
                None
            } else {
                Url::parse(env::WEAVER_TOS_URI).ok()
            };
            let privacy_policy_uri = if env::WEAVER_PRIVACY_POLICY_URI.is_empty() {
                None
            } else {
                Url::parse(env::WEAVER_PRIVACY_POLICY_URI).ok()
            };
            Self::new(
                Url::parse(&client_id).expect("Failed to parse client ID as valid URL"),
                Url::parse(&redirect_uri).expect("Failed to parse redirect URI as valid URL"),
                Scope::parse_multiple(env::WEAVER_APP_SCOPES)
                    .unwrap_or(vec![])
                    .into_static(),
                env::WEAVER_CLIENT_NAME.to_smolstr(),
                Some(Url::parse(&host).expect("Failed to parse host as valid URL")),
                logo_uri,
                tos_uri,
                privacy_policy_uri,
            )
        }
    }

    pub fn as_metadata(self) -> AtprotoClientMetadata<'static> {
        AtprotoClientMetadata::new(
            self.client_id,
            self.client_uri,
            vec![self.redirect_uri],
            vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
            self.scopes,
            None,
        )
        .with_prod_info(
            self.client_name.as_str(),
            self.logo_uri,
            self.tos_uri,
            self.privacy_policy_uri,
        )
    }
}

#[derive(PartialEq)]
enum AppEnv {
    Dev,
    Prod,
}

impl std::str::FromStr for AppEnv {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dev" => Ok(Self::Dev),
            "prod" => Ok(Self::Prod),
            s => Err(format!("Invalid AppEnv: {s}")),
        }
    }
}

impl fmt::Display for AppEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppEnv::Dev => write!(f, "dev"),
            AppEnv::Prod => write!(f, "prod"),
        }
    }
}
