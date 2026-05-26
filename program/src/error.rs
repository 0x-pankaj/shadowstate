//! Program error type. A plain `#[repr(u32)]` enum mapped into `ProgramError::Custom` — no
//! `thiserror`/`num_derive` dependency, keeping the on-chain crate dependency-light per the
//! zero-bloat mandate. Every settlement-path failure returns one of these explicitly; the engine
//! never `unwrap()`s, `expect()`s, or `panic!`s on a fallible path.

use pinocchio::error::ProgramError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ShadowError {
    /// Instruction data was shorter than the fixed layout requires.
    InvalidInstructionData = 0,
    /// Account data failed a discriminator, length, or alignment check.
    InvalidAccountData = 1,
    /// A required signer did not sign.
    MissingSignature = 2,
    /// A passed account's address did not match its expected PDA derivation.
    InvalidPda = 3,
    /// A passed program / mint / token account did not match the expected id.
    InvalidAccount = 4,
    /// `header.epoch <= market.last_epoch` — the batch is a replay or out of order.
    StaleEpoch = 5,
    /// Fewer than `Committee.threshold` distinct committee members signed the frame.
    InsufficientCommitteeSignatures = 6,
    /// The recomputed economics (net imbalance / direction) disagree with the header.
    FrameEconomicsMismatch = 7,
    /// `header.fill_count` exceeds `MAX_FILLS` — would exceed the bounded settlement loop.
    TooManyFills = 8,
    /// A checked arithmetic operation overflowed/underflowed.
    MathOverflow = 9,
    /// Committee configuration in the instruction was malformed (count/threshold invalid).
    InvalidCommitteeConfig = 10,
    /// Risk parameters were out of their permitted bounds.
    InvalidRiskParams = 11,
    /// User has insufficient deposited collateral to cover a debit.
    InsufficientCollateral = 12,
    /// No Ed25519 precompile verification instruction was found in the transaction.
    MissingSignatureProgram = 13,
    /// MM has not posted enough backstop collateral to fully collateralize a Tier-2 residual.
    InsufficientMmCollateral = 14,
    /// A resolution/claim instruction ran on a market that is not yet resolved.
    MarketNotResolved = 15,
    /// `ResolveMarket` ran on a market that was already resolved (outcomes are final).
    MarketAlreadyResolved = 16,
    /// The proposed outcome byte was not a valid `OUTCOME_YES_WON` / `OUTCOME_NO_WON`.
    InvalidOutcome = 17,
    /// A claim found nothing payable (no winning contracts, or already claimed).
    NothingToClaim = 18,
    /// Trusted-gateway settlement was attempted but the signer is not the registered settlement
    /// authority, or the market has no settlement authority configured.
    UnauthorizedSettlement = 19,
    /// Settlement was attempted on a market whose trading window is closed.
    TradingClosed = 20,
    /// Resolution / close was attempted in the wrong lifecycle state (e.g. resolve before close).
    MarketNotClosed = 21,
}

impl From<ShadowError> for ProgramError {
    fn from(e: ShadowError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
