//! Cross-platform clearing: dispatch the offsetting hedge order to an external lit venue.
//!
//! [`VenueClient`] is the abstraction over a programmatic trading venue (Polymarket / Kalshi / a
//! Web2 sportsbook). [`HttpVenueClient`] is a real `reqwest` adapter that POSTs the order as JSON;
//! [`MockVenue`] records orders for offline tests. The hedging *decision* (see [`crate::hedge`]) is
//! venue-agnostic — only this layer knows the wire format.

use {
    crate::{
        error::{GatewayError, Result},
        hedge::{HedgeOrder, HedgeSide},
    },
    serde::{Deserialize, Serialize},
    std::{future::Future, sync::Mutex},
};

/// A venue's acknowledgement of a placed order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueOrderAck {
    /// The venue's order identifier.
    pub venue_order_id: String,
    /// Quantity the venue reports filled (may be partial).
    pub filled_qty: u64,
}

/// Wire body POSTed to a venue. Side is the human-readable contract side; the market id is a stable
/// opaque token (base64 of the 32-byte market key) the venue maps to its own symbol.
#[derive(Debug, Serialize)]
struct VenueOrderRequest<'a> {
    market: String,
    side: &'a str,
    qty: u64,
    limit_price: u64,
    epoch: u64,
}

fn side_str(side: HedgeSide) -> &'static str {
    match side {
        HedgeSide::Yes => "YES",
        HedgeSide::No => "NO",
    }
}

fn market_token(market: &[u8; 32]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.encode(market)
}

/// A programmatic external trading venue.
pub trait VenueClient {
    /// Human-readable venue name (for logging / correlation).
    fn name(&self) -> &str;
    /// Place an offsetting order and await the venue's acknowledgement.
    fn place(&self, order: &HedgeOrder) -> impl Future<Output = Result<VenueOrderAck>>;
}

/// Real HTTP venue adapter: `POST {base_url}/orders` with the order as JSON.
pub struct HttpVenueClient {
    name: String,
    base_url: String,
    client: reqwest::Client,
}

impl HttpVenueClient {
    /// Build a client for `name` against `base_url` (e.g. `https://clob.polymarket.com`).
    pub fn new(name: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl VenueClient for HttpVenueClient {
    fn name(&self) -> &str {
        &self.name
    }

    async fn place(&self, order: &HedgeOrder) -> Result<VenueOrderAck> {
        let body = VenueOrderRequest {
            market: market_token(&order.market),
            side: side_str(order.side),
            qty: order.qty,
            limit_price: order.limit_price,
            epoch: order.epoch,
        };
        let resp = self
            .client
            .post(format!("{}/orders", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| GatewayError::Venue(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(GatewayError::Venue(format!("venue returned status {}", resp.status())));
        }
        resp.json::<VenueOrderAck>()
            .await
            .map_err(|e| GatewayError::Venue(e.to_string()))
    }
}

/// In-memory venue for tests: records every placed order and fully fills it.
pub struct MockVenue {
    name: String,
    placed: Mutex<Vec<HedgeOrder>>,
}

impl MockVenue {
    /// A mock venue with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            placed: Mutex::new(Vec::new()),
        }
    }

    /// Snapshot of the orders placed so far, in order.
    pub fn placed(&self) -> Vec<HedgeOrder> {
        self.placed.lock().expect("venue mutex").clone()
    }
}

impl VenueClient for MockVenue {
    fn name(&self) -> &str {
        &self.name
    }

    async fn place(&self, order: &HedgeOrder) -> Result<VenueOrderAck> {
        self.placed.lock().expect("venue mutex").push(*order);
        Ok(VenueOrderAck {
            venue_order_id: format!("mock-{}-{}", self.name, order.epoch),
            filled_qty: order.qty,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order() -> HedgeOrder {
        HedgeOrder {
            market: [7u8; 32],
            epoch: 3,
            side: HedgeSide::Yes,
            qty: 200,
            limit_price: 520_000,
            captured_premium: 20_000,
        }
    }

    #[tokio::test]
    async fn mock_venue_records_and_acks() {
        let venue = MockVenue::new("polymarket");
        let ack = venue.place(&order()).await.unwrap();
        assert_eq!(ack.filled_qty, 200);
        assert_eq!(ack.venue_order_id, "mock-polymarket-3");
        assert_eq!(venue.placed().len(), 1);
        assert_eq!(venue.placed()[0].qty, 200);
    }

    #[test]
    fn side_and_market_token_serialize_stably() {
        assert_eq!(side_str(HedgeSide::Yes), "YES");
        assert_eq!(side_str(HedgeSide::No), "NO");
        // base64 of 32 zero bytes is a stable 44-char token.
        assert_eq!(market_token(&[0u8; 32]).len(), 44);
    }
}
