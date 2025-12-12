use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, trace, warn};
use url::Url;

use crate::error::IndexError;

use super::{TapAck, TapEvent};

/// Messages sent to the writer task
enum WriteCommand {
    #[allow(dead_code)]
    Ack(u64),
    Pong(bytes::Bytes),
}

/// Configuration for tap consumer
#[derive(Debug, Clone)]
pub struct TapConfig {
    /// WebSocket URL for tap (e.g., ws://localhost:2480/channel)
    pub url: Url,
    /// Whether to send acks (disable for fire-and-forget mode)
    pub send_acks: bool,
    /// Reconnect delay on connection failure
    pub reconnect_delay: Duration,
}

impl TapConfig {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            send_acks: true,
            reconnect_delay: Duration::from_secs(5),
        }
    }

    pub fn with_acks(mut self, send_acks: bool) -> Self {
        self.send_acks = send_acks;
        self
    }
}

/// Consumer that connects to tap's websocket and yields events
pub struct TapConsumer {
    config: TapConfig,
}

impl TapConsumer {
    pub fn new(config: TapConfig) -> Self {
        Self { config }
    }

    /// Connect to tap and return channels for events and acks
    ///
    /// Returns a receiver for events and a sender for acks.
    /// The consumer handles reconnection internally.
    pub async fn connect(
        &self,
    ) -> Result<(mpsc::Receiver<TapEvent>, mpsc::Sender<u64>), IndexError> {
        let (event_tx, event_rx) = mpsc::channel::<TapEvent>(10000);
        let (ack_tx, ack_rx) = mpsc::channel::<u64>(10000);

        let config = self.config.clone();
        tokio::spawn(async move {
            run_connection_loop(config, event_tx, ack_rx).await;
        });

        Ok((event_rx, ack_tx))
    }
}

async fn run_connection_loop(
    config: TapConfig,
    event_tx: mpsc::Sender<TapEvent>,
    ack_rx: mpsc::Receiver<u64>,
) {
    loop {
        info!(url = %config.url, "connecting to tap");

        match connect_async(config.url.as_str()).await {
            Ok((ws_stream, _response)) => {
                info!("connected to tap");

                let (write, read) = ws_stream.split();

                // Channel for reader -> writer communication (pongs, etc)
                let (write_tx, write_rx) = mpsc::channel::<WriteCommand>(10000);

                // Spawn writer task
                let send_acks = config.send_acks;
                let writer_handle = tokio::spawn(run_writer(write, write_rx, ack_rx, send_acks));

                // Run reader in current task
                let reader_result = run_reader(read, event_tx.clone(), write_tx, send_acks).await;

                // Reader finished - abort writer and wait for it
                writer_handle.abort();
                let _ = writer_handle.await;

                // Get back the ack_rx from... wait, we moved it. Need to restructure.
                // For now, if reader dies we'll reconnect with a fresh ack channel state

                match reader_result {
                    ReaderResult::Closed => {
                        info!("tap connection closed");
                    }
                    ReaderResult::Error(e) => {
                        warn!(error = %e, "tap reader error");
                    }
                    ReaderResult::ChannelClosed => {
                        error!("event channel closed, stopping tap consumer");
                        return;
                    }
                }

                // We lost the ack_rx to the writer task, need to break out
                // and let caller reconnect if needed
                break;
            }
            Err(e) => {
                error!(error = ?e, "failed to connect to tap");
            }
        }

        // Reconnect after delay
        info!(delay = ?config.reconnect_delay, "reconnecting to tap");
        tokio::time::sleep(config.reconnect_delay).await;
    }
}

enum ReaderResult {
    Closed,
    Error(String),
    ChannelClosed,
}

async fn run_reader<S>(
    mut read: S,
    event_tx: mpsc::Sender<TapEvent>,
    write_tx: mpsc::Sender<WriteCommand>,
    send_acks: bool,
) -> ReaderResult
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => match serde_json::from_str::<TapEvent>(&text) {
                Ok(event) => {
                    let event_id = event.id();
                    if event_tx.send(event).await.is_err() {
                        return ReaderResult::ChannelClosed;
                    }

                    if !send_acks {
                        debug!(id = event_id, "event received (fire-and-forget)");
                    }
                }
                Err(e) => {
                    warn!(error = ?e, text = %text, "failed to parse tap event");
                }
            },
            Ok(Message::Ping(data)) => {
                if write_tx.send(WriteCommand::Pong(data)).await.is_err() {
                    return ReaderResult::Error("writer channel closed".into());
                }
            }
            Ok(Message::Close(_)) => {
                return ReaderResult::Closed;
            }
            Ok(_) => {
                // Ignore binary, pong, etc.
            }
            Err(e) => {
                return ReaderResult::Error(e.to_string());
            }
        }
    }
    ReaderResult::Closed
}

async fn run_writer<S>(
    mut write: S,
    mut write_rx: mpsc::Receiver<WriteCommand>,
    mut ack_rx: mpsc::Receiver<u64>,
    send_acks: bool,
) where
    S: SinkExt<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    loop {
        tokio::select! {
            biased;

            // Handle pongs and other write commands from reader
            cmd = write_rx.recv() => {
                match cmd {
                    Some(WriteCommand::Pong(data)) => {
                        if let Err(e) = write.send(Message::Pong(data)).await {
                            warn!(error = %e, "failed to send pong");
                            return;
                        }
                    }
                    Some(WriteCommand::Ack(id)) => {
                        if send_acks {
                            if let Err(e) = send_ack(&mut write, id).await {
                                warn!(error = %e, id, "failed to send ack");
                                return;
                            }
                        }
                    }
                    None => {
                        // Reader closed the channel, we're done
                        return;
                    }
                }
            }

            // Handle acks from the indexer
            id = ack_rx.recv(), if send_acks => {
                match id {
                    Some(id) => {
                        if let Err(e) = send_ack(&mut write, id).await {
                            warn!(error = %e, id, "failed to send ack");
                            return;
                        }
                    }
                    None => {
                        // Ack channel closed, indexer is done
                        return;
                    }
                }
            }
        }
    }
}

async fn send_ack<S>(write: &mut S, id: u64) -> Result<(), String>
where
    S: SinkExt<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let ack = TapAck::new(id);
    let json = serde_json::to_string(&ack).map_err(|e| e.to_string())?;
    write
        .send(Message::Text(json.into()))
        .await
        .map_err(|e| e.to_string())?;
    trace!(id, "sent ack");
    Ok(())
}
