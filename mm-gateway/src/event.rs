//! Settlement events — the MM's view of each finalized batch.
//!
//! The gateway learns a batch settled either by reading the on-chain `SubmitBatch` frame bytes or by
//! receiving an Arcium log line carrying the same frame (base64) over WebSocket. Both decode into a
//! [`SettlementEvent`], which carries exactly the fields the delta-hedger needs: the direction and
//! size of the residual the MM was forced to backstop.

use {
    crate::error::{GatewayError, Result},
    base64::{engine::general_purpose::STANDARD, Engine},
    protocol::validate_frame_len,
    serde::Deserialize,
};

/// One batch's settlement outcome, from the MM's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettlementEvent {
    /// Market the batch settled against.
    pub market: [u8; 32],
    /// FBA epoch (strictly increasing).
    pub epoch: u64,
    /// Batch id.
    pub batch_id: u64,
    /// `DIRECTION_YES_HEAVY` / `DIRECTION_NO_HEAVY` — which side the *flow* was heavy on. The MM
    /// took the opposite side.
    pub direction: u8,
    /// Residual size the MM backstopped (`net_imbalance`).
    pub net_imbalance: u64,
    /// Total P2P-crossed volume in the batch (informational).
    pub p2p_volume: u64,
}

/// JSON shape of an Arcium settlement log line: `{"frame": "<base64 of the on-chain frame>"}`.
#[derive(Debug, Deserialize)]
struct FrameLog {
    frame: String,
}

impl SettlementEvent {
    /// Parse from the raw on-chain frame bytes (`BatchHeader ++ [UserFill]`). Runs the same length
    /// validation the on-chain engine runs before trusting the header.
    pub fn from_frame(frame: &[u8]) -> Result<Self> {
        let header = validate_frame_len(frame).ok_or(GatewayError::MalformedEvent)?;
        Ok(SettlementEvent {
            market: header.market,
            epoch: header.epoch,
            batch_id: header.batch_id,
            direction: header.direction,
            net_imbalance: header.net_imbalance,
            p2p_volume: header.p2p_volume,
        })
    }

    /// Parse from an Arcium WS log line: a JSON object `{"frame": "<base64>"}`.
    pub fn from_log_json(text: &str) -> Result<Self> {
        let log: FrameLog = serde_json::from_str(text).map_err(|_| GatewayError::MalformedEvent)?;
        let frame = STANDARD
            .decode(log.frame.as_bytes())
            .map_err(|_| GatewayError::MalformedEvent)?;
        Self::from_frame(&frame)
    }

    /// `true` if the MM had to backstop a residual (i.e. a hedge is warranted).
    #[inline]
    pub fn has_residual(&self) -> bool {
        self.net_imbalance > 0
    }
}

#[cfg(test)]
mod tests {
    use {super::*, bytemuck::bytes_of, protocol::BatchHeader};

    fn sample_frame(direction: u8, net: u64) -> Vec<u8> {
        // Header-only frame (fill_count = 0) is a valid frame for event purposes.
        let header = BatchHeader {
            market: [7u8; 32],
            epoch: 5,
            batch_id: 5,
            p2p_volume: 1_000,
            net_imbalance: net,
            fill_count: 0,
            direction,
            _pad: [0; 5],
        };
        bytes_of(&header).to_vec()
    }

    #[test]
    fn parses_from_frame() {
        let frame = sample_frame(protocol::DIRECTION_YES_HEAVY, 200);
        let ev = SettlementEvent::from_frame(&frame).unwrap();
        assert_eq!(ev.market, [7u8; 32]);
        assert_eq!(ev.epoch, 5);
        assert_eq!(ev.direction, protocol::DIRECTION_YES_HEAVY);
        assert_eq!(ev.net_imbalance, 200);
        assert!(ev.has_residual());
    }

    #[test]
    fn parses_from_log_json() {
        let frame = sample_frame(protocol::DIRECTION_NO_HEAVY, 50);
        let b64 = STANDARD.encode(&frame);
        let line = format!("{{\"frame\":\"{b64}\"}}");
        let ev = SettlementEvent::from_log_json(&line).unwrap();
        assert_eq!(ev.direction, protocol::DIRECTION_NO_HEAVY);
        assert_eq!(ev.net_imbalance, 50);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(SettlementEvent::from_frame(&[0u8; 3]), Err(GatewayError::MalformedEvent));
        assert_eq!(SettlementEvent::from_log_json("not json"), Err(GatewayError::MalformedEvent));
        assert_eq!(
            SettlementEvent::from_log_json("{\"frame\":\"!!!notb64\"}"),
            Err(GatewayError::MalformedEvent)
        );
    }

    #[test]
    fn empty_residual_needs_no_hedge() {
        let frame = sample_frame(protocol::DIRECTION_YES_HEAVY, 0);
        assert!(!SettlementEvent::from_frame(&frame).unwrap().has_residual());
    }
}
