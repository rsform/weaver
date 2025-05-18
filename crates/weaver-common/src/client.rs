use atrium_api::app::bsky::actor::defs::ProfileViewDetailedData;
use atrium_api::{agent::SessionManager, types::string::AtIdentifier};

use crate::Error;

pub struct WeaverHttpClient {
    pub client: reqwest::Client,
}

impl atrium_xrpc::HttpClient for WeaverHttpClient {
    async fn send_http(
        &self,
        request: atrium_xrpc::http::Request<Vec<u8>>,
    ) -> core::result::Result<
        atrium_xrpc::http::Response<Vec<u8>>,
        Box<dyn std::error::Error + Send + Sync + 'static>,
    > {
        let response = self.client.execute(request.try_into()?).await?;
        let mut builder = atrium_xrpc::http::Response::builder().status(response.status());
        for (k, v) in response.headers() {
            builder = builder.header(k, v);
        }
        builder
            .body(response.bytes().await?.to_vec())
            .map_err(Into::into)
    }
}

impl Default for WeaverHttpClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

pub struct WeaverAgent<M>
where
    M: SessionManager + Send + Sync,
{
    pub a: atrium_api::agent::Agent<M>,
}

impl<M> WeaverAgent<M>
where
    M: SessionManager + Send + Sync,
{
    pub fn new(session_manager: M) -> Self {
        Self {
            a: atrium_api::agent::Agent::new(session_manager),
        }
    }

    pub async fn get_profile(&self, actor: AtIdentifier) -> Result<ProfileViewDetailedData, Error> {
        use atrium_api::app::bsky::actor::get_profile::*;
        let resp = self
            .a
            .api
            .app
            .bsky
            .actor
            .get_profile(ParametersData { actor }.into())
            .await?;

        Ok(resp.data)
    }
}
