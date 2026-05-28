//! The event stream: where settlement outcomes come from.
//!
//! [`EventStream`] abstracts the source so the gateway loop is testable with [`MemoryStream`].
//! [`WsEventStream`] is the real `tokio-tungstenite` worker that hooks an Arcium network log stream
//! over WebSocket and decodes each settlement log line into a [`SettlementEvent`]. Non-settlement
//! log lines are skipped; a clean close ends the stream.

use {
    crate::{error::{GatewayError, Result}, event::SettlementEvent},
    std::{collections::VecDeque, future::Future},
};

/// A source of settlement events. `next_event` resolves to `None` when the stream ends.
pub trait EventStream {
    /// Await the next settlement event, or `None` if the stream has closed.
    fn next_event(&mut self) -> impl Future<Output = Result<Option<SettlementEvent>>>;
}

/// In-memory event stream for tests: yields queued events, then `None`.
pub struct MemoryStream {
    events: VecDeque<SettlementEvent>,
}

impl MemoryStream {
    /// Wrap a fixed sequence of events.
    pub fn new(events: Vec<SettlementEvent>) -> Self {
        Self {
            events: events.into(),
        }
    }
}

impl EventStream for MemoryStream {
    async fn next_event(&mut self) -> Result<Option<SettlementEvent>> {
        Ok(self.events.pop_front())
    }
}

// ---- real WebSocket worker --------------------------------------------------------------------

use {
    futures_util::StreamExt,
    tokio::net::TcpStream,
    tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream},
};

/// Real Arcium-log WebSocket worker. Each text frame is expected to be a settlement log line
/// (`{"frame":"<base64>"}`); other frames (pings, non-settlement logs) are skipped.
pub struct WsEventStream {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl WsEventStream {
    /// Connect to an Arcium log-stream WebSocket endpoint (`ws://` or `wss://`).
    pub async fn connect(url: &str) -> Result<Self> {
        let (ws, _resp) = connect_async(url)
            .await
            .map_err(|e| GatewayError::Stream(e.to_string()))?;
        Ok(Self { ws })
    }
}

impl EventStream for WsEventStream {
    async fn next_event(&mut self) -> Result<Option<SettlementEvent>> {
        while let Some(msg) = self.ws.next().await {
            let msg = msg.map_err(|e| GatewayError::Stream(e.to_string()))?;
            match msg {
                Message::Text(text) => {
                    // Skip log lines that aren't settlement frames; return the first that parses.
                    match SettlementEvent::from_log_json(text.as_ref()) {
                        Ok(ev) => return Ok(Some(ev)),
                        Err(_) => continue,
                    }
                }
                Message::Close(_) => return Ok(None),
                // Ignore binary / ping / pong / raw frames and await the next message.
                _ => continue,
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(epoch: u64) -> SettlementEvent {
        SettlementEvent {
            market: [7u8; 32],
            epoch,
            batch_id: epoch,
            direction: protocol::DIRECTION_YES_HEAVY,
            net_imbalance: 100,
            p2p_volume: 0,
        }
    }

    #[tokio::test]
    async fn memory_stream_drains_then_ends() {
        let mut s = MemoryStream::new(vec![ev(1), ev(2)]);
        assert_eq!(s.next_event().await.unwrap().unwrap().epoch, 1);
        assert_eq!(s.next_event().await.unwrap().unwrap().epoch, 2);
        assert!(s.next_event().await.unwrap().is_none());
    }
}
