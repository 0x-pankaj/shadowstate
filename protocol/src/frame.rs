//! The signed batch data-frame: the exact byte payload the Arcium MPC committee signs and
//! the on-chain engine executes.
//!
//! Wire layout (little-endian, packed with explicit padding — NO implicit padding):
//! ```text
//!   [ BatchHeader : 72 bytes ] [ UserFill : 64 bytes ] * header.fill_count
//! ```
//! The committee signs exactly these bytes (header ++ fills). On-chain, the `SubmitBatch`
//! instruction data is `[disc:u8=3] ++ <this frame>`, and the signed message is the frame
//! slice with the discriminator byte stripped.
//!
//! Alignment note: instruction data is NOT guaranteed 8-byte aligned by the SVM loader, so the
//! frame is parsed with *unaligned* `pod_read_unaligned` copies rather than a zero-copy cast.
//! (Account *state* — see the program's `state.rs` — lives in 8-aligned account data and IS
//! cast zero-copy.) Both structs are `#[repr(C)]` so the off-chain side can serialize them with
//! `bytemuck::bytes_of` and get byte-identical output.

use bytemuck::{Pod, Zeroable};

/// Per-batch summary produced by the MPC aggregation/matching circuit.
///
/// `net_imbalance` / `direction` are advisory: the on-chain engine recomputes them from the
/// fills and rejects the batch on mismatch. Only the committee signature over the raw bytes is
/// trusted — never the header's economics in isolation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct BatchHeader {
    /// MarketState PDA this frame settles against (binds the frame to one market).
    pub market: [u8; 32],
    /// Strictly-increasing FBA epoch. On-chain replay guard requires `epoch > last_epoch`.
    pub epoch: u64,
    /// Monotonic batch identifier (for off-chain bookkeeping / event correlation).
    pub batch_id: u64,
    /// Total volume crossed peer-to-peer at `MIDPOINT_PRICE` (Tier 1), in contract units.
    pub p2p_volume: u64,
    /// `|Σ residual_yes − Σ residual_no|` — the one-sided remainder the MM backstops (Tier 2).
    pub net_imbalance: u64,
    /// `fill_count` declared length of the trailing `UserFill` array.
    pub fill_count: u16,
    /// `DIRECTION_YES_HEAVY` or `DIRECTION_NO_HEAVY` (see `constants`).
    pub direction: u8,
    /// Explicit tail padding → struct size is a multiple of its 8-byte alignment, no implicit
    /// padding bytes (required for `Pod` and for deterministic cross-machine serialization).
    pub _pad: [u8; 5],
}

/// One participant's settled amounts for the batch. Quantities only — the engine derives all
/// collateral movement from these plus the on-chain-computed clearing price.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct UserFill {
    /// Owner whose `UserPosition` PDA is updated. Must match the position account passed in the
    /// same ordinal slot of the instruction's account list.
    pub user: [u8; 32],
    /// YES contracts filled peer-to-peer at `MIDPOINT_PRICE` (Tier 1).
    pub p2p_yes: u64,
    /// NO contracts filled peer-to-peer at `MIDPOINT_PRICE` (Tier 1).
    pub p2p_no: u64,
    /// YES contracts filled from the residual imbalance at the Tier-2 clearing price.
    /// Non-zero only when `direction == DIRECTION_YES_HEAVY`.
    pub residual_yes: u64,
    /// NO contracts filled from the residual imbalance at the Tier-2 clearing price.
    /// Non-zero only when `direction == DIRECTION_NO_HEAVY`.
    pub residual_no: u64,
}

/// Serialized size of `BatchHeader`.
pub const HEADER_LEN: usize = core::mem::size_of::<BatchHeader>();
/// Serialized size of one `UserFill`.
pub const FILL_LEN: usize = core::mem::size_of::<UserFill>();

// Frozen-layout guarantees. A mismatch here is a compile error, catching accidental padding or
// field reordering before it can corrupt the wire format.
const _: () = assert!(HEADER_LEN == 72, "BatchHeader must be 72 bytes");
const _: () = assert!(FILL_LEN == 64, "UserFill must be 64 bytes");
const _: () = assert!(core::mem::align_of::<BatchHeader>() == 8);
const _: () = assert!(core::mem::align_of::<UserFill>() == 8);

/// Total signed-message / frame length for `n` fills. Returns `None` on overflow.
pub const fn frame_len(fill_count: u16) -> usize {
    HEADER_LEN + (fill_count as usize) * FILL_LEN
}

/// Read the header from the front of `frame` via an unaligned copy. `None` if too short or the
/// byte pattern is somehow invalid (cannot happen for an all-bits-valid `Pod`, but checked).
pub fn read_header(frame: &[u8]) -> Option<BatchHeader> {
    let bytes = frame.get(..HEADER_LEN)?;
    bytemuck::try_pod_read_unaligned(bytes).ok()
}

/// Read the `i`-th `UserFill` via an unaligned copy. `None` if out of range.
pub fn read_fill(frame: &[u8], i: usize) -> Option<UserFill> {
    let start = HEADER_LEN.checked_add(i.checked_mul(FILL_LEN)?)?;
    let end = start.checked_add(FILL_LEN)?;
    let bytes = frame.get(start..end)?;
    bytemuck::try_pod_read_unaligned(bytes).ok()
}

/// Validate that `frame` is exactly a header plus `header.fill_count` fills with nothing extra,
/// returning the parsed header. This is the canonical length check the engine runs before
/// trusting any per-fill read.
pub fn validate_frame_len(frame: &[u8]) -> Option<BatchHeader> {
    let header = read_header(frame)?;
    if frame.len() == frame_len(header.fill_count) {
        Some(header)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrips_through_bytes() {
        let h = BatchHeader {
            market: [7u8; 32],
            epoch: 42,
            batch_id: 9,
            p2p_volume: 1_000,
            net_imbalance: 250,
            fill_count: 2,
            direction: 0,
            _pad: [0; 5],
        };
        let bytes = bytemuck::bytes_of(&h);
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(read_header(bytes).unwrap(), h);
    }

    #[test]
    fn validate_frame_len_checks_exact_size() {
        let h = BatchHeader {
            market: [0u8; 32],
            epoch: 1,
            batch_id: 0,
            p2p_volume: 0,
            net_imbalance: 0,
            fill_count: 1,
            direction: 0,
            _pad: [0; 5],
        };
        let fill = UserFill {
            user: [1u8; 32],
            p2p_yes: 5,
            p2p_no: 0,
            residual_yes: 0,
            residual_no: 0,
        };
        let mut frame = Vec::new();
        frame.extend_from_slice(bytemuck::bytes_of(&h));
        frame.extend_from_slice(bytemuck::bytes_of(&fill));
        assert_eq!(validate_frame_len(&frame).unwrap().fill_count, 1);
        assert_eq!(read_fill(&frame, 0).unwrap(), fill);

        frame.push(0); // one trailing byte → invalid
        assert!(validate_frame_len(&frame).is_none());
    }
}
