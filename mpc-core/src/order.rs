//! The plaintext order and its fixed on-the-wire byte layout.
//!
//! A client constructs an [`Order`], serializes it to its canonical 81-byte form, and **seals** it
//! (see [`crate::seal`]) into an ingestion account. The MXE node matrix fetches the sealed bytes
//! over RPC, opens them only inside the secure context, and immediately splits the quantities into
//! secret shares — so the plaintext order exists only transiently at the trust boundary.

use crate::error::{MpcError, Result};

/// Which side of the binary market the order takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Side {
    /// Buying YES contracts (pays out 1.0 if the event resolves true).
    Yes = 0,
    /// Buying NO contracts (pays out 1.0 if the event resolves false).
    No = 1,
}

impl Side {
    fn from_u8(b: u8) -> Result<Self> {
        match b {
            0 => Ok(Side::Yes),
            1 => Ok(Side::No),
            _ => Err(MpcError::MalformedOrder),
        }
    }
}

/// A single client order. `limit_price` is the worst 6-decimal fixed-point price the client will
/// accept (e.g. `550_000` = $0.55); `0` means "take the batch clearing price unconditionally".
/// The on-chain settlement is plaintext, but the *order* (who, which side, how much, limit) is
/// private until matched — that privacy is what this crate protects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    /// Owner address — the `UserPosition` PDA owner this order settles into.
    pub user: [u8; 32],
    /// Market this order belongs to (binds it to one `MarketState`).
    pub market: [u8; 32],
    /// YES or NO.
    pub side: Side,
    /// Contract quantity (same units as `UserFill` amounts).
    pub qty: u64,
    /// Client limit price (6-dec fixed point); `0` = market.
    pub limit_price: u64,
}

/// Canonical serialized length of an [`Order`]: 32 + 32 + 1 + 8 + 8.
pub const ORDER_WIRE_LEN: usize = 32 + 32 + 1 + 8 + 8;

impl Order {
    /// Serialize to the canonical little-endian byte layout (the bytes that get sealed).
    pub fn to_bytes(&self) -> [u8; ORDER_WIRE_LEN] {
        let mut out = [0u8; ORDER_WIRE_LEN];
        out[0..32].copy_from_slice(&self.user);
        out[32..64].copy_from_slice(&self.market);
        out[64] = self.side as u8;
        out[65..73].copy_from_slice(&self.qty.to_le_bytes());
        out[73..81].copy_from_slice(&self.limit_price.to_le_bytes());
        out
    }

    /// Parse from the canonical byte layout. Rejects wrong length or an invalid side tag.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != ORDER_WIRE_LEN {
            return Err(MpcError::MalformedOrder);
        }
        let mut user = [0u8; 32];
        let mut market = [0u8; 32];
        user.copy_from_slice(&bytes[0..32]);
        market.copy_from_slice(&bytes[32..64]);
        let side = Side::from_u8(bytes[64])?;
        // `try_into` on a fixed slice cannot fail given the length check above, but stay explicit.
        let qty = u64::from_le_bytes(bytes[65..73].try_into().map_err(|_| MpcError::MalformedOrder)?);
        let limit_price =
            u64::from_le_bytes(bytes[73..81].try_into().map_err(|_| MpcError::MalformedOrder)?);
        Ok(Order {
            user,
            market,
            side,
            qty,
            limit_price,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_roundtrips_through_wire_bytes() {
        let o = Order {
            user: [9u8; 32],
            market: [4u8; 32],
            side: Side::No,
            qty: 123_456,
            limit_price: 480_000,
        };
        let bytes = o.to_bytes();
        assert_eq!(bytes.len(), ORDER_WIRE_LEN);
        assert_eq!(Order::from_bytes(&bytes).unwrap(), o);
    }

    #[test]
    fn rejects_bad_side_and_length() {
        let mut bytes = Order {
            user: [0u8; 32],
            market: [0u8; 32],
            side: Side::Yes,
            qty: 1,
            limit_price: 0,
        }
        .to_bytes();
        bytes[64] = 7; // invalid side
        assert_eq!(Order::from_bytes(&bytes), Err(MpcError::MalformedOrder));
        assert_eq!(Order::from_bytes(&bytes[..80]), Err(MpcError::MalformedOrder));
    }
}
