use crate::config::FirehoseConfig;
use crate::error::{FirehoseError, IndexError};
use jacquard_common::stream::StreamError;
use jacquard_common::xrpc::subscription::{SubscriptionClient, TungsteniteSubscriptionClient};
use n0_future::stream::Boxed;

// Re-export the message types from weaver_api for convenience
pub use weaver_api::com_atproto::sync::subscribe_repos::{
    Account, Commit, Identity, SubscribeRepos, SubscribeReposMessage, Sync,
};

/// Typed firehose message stream
pub type MessageStream = Boxed<Result<SubscribeReposMessage<'static>, StreamError>>;

/// Firehose consumer that connects to a relay and yields typed events
pub struct FirehoseConsumer {
    config: FirehoseConfig,
}

impl FirehoseConsumer {
    pub fn new(config: FirehoseConfig) -> Self {
        Self { config }
    }

    /// Connect to the firehose and return a typed message stream
    ///
    /// Messages are automatically decoded and converted to owned ('static) types.
    pub async fn connect(&self) -> Result<MessageStream, IndexError> {
        let client = TungsteniteSubscriptionClient::from_base_uri(self.config.relay_url.clone());

        let mut params = SubscribeRepos::new();
        if let Some(cursor) = self.config.cursor {
            params = params.cursor(cursor);
        }
        let params = params.build();

        let stream = client
            .subscribe(&params)
            .await
            .map_err(|e| FirehoseError::Connection {
                url: self.config.relay_url.to_string(),
                message: e.to_string(),
            })?;

        let (_sink, messages) = stream.into_stream();
        Ok(messages)
    }
}
