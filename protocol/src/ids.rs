//! Stable instruction and account discriminators. Single-byte (Pinocchio convention — not
//! Anchor's 8-byte hashes). Part of the frozen wire contract.

/// On-chain account version stamped into every state account's `version` field. Bump when a
/// `#[repr(C)]` layout changes so old accounts can be detected and migrated.
pub const ACCOUNT_VERSION: u8 = 1;

/// Instruction discriminators (instruction-data byte 0).
pub mod ix {
    pub const INITIALIZE_MARKET: u8 = 0;
    pub const INIT_USER_POSITION: u8 = 1;
    pub const DEPOSIT_COLLATERAL: u8 = 2;
    pub const SUBMIT_BATCH: u8 = 3;
    pub const UPDATE_RISK_PARAMS: u8 = 4;
    pub const WITHDRAW_COLLATERAL: u8 = 5;
    /// MM funds the backstop collateral pool (vault) that fully collateralizes Tier-2 residuals.
    pub const DEPOSIT_MM_COLLATERAL: u8 = 6;
    /// Resolver authority declares the market outcome (YES or NO won).
    pub const RESOLVE_MARKET: u8 = 7;
    /// A user claims $1/contract for their winning side after resolution.
    pub const CLAIM_WINNINGS: u8 = 8;
    /// The MM claims $1/contract for its winning backstop side after resolution.
    pub const CLAIM_MM_WINNINGS: u8 = 9;
    /// Settle a batch via the trusted gateway authority (no committee signatures); the trust anchor
    /// is the Arcium MXE attestation verified off-chain in the `arcium-gateway`.
    pub const SUBMIT_BATCH_TRUSTED: u8 = 10;
    /// Close trading on a market (authority); no further batches settle, resolution becomes possible.
    pub const CLOSE_MARKET: u8 = 11;
    /// MM reclaims its unreserved backstop float from the vault.
    pub const WITHDRAW_MM_COLLATERAL: u8 = 12;
}

/// Account discriminators (account-data byte 0). `0` is reserved as the "uninitialized" sentinel
/// (freshly created accounts are zeroed), so real types start at `1` — a zeroed buffer can never
/// be mistaken for an initialized account of any type.
pub mod account {
    pub const UNINITIALIZED: u8 = 0;
    pub const MARKET_STATE: u8 = 1;
    pub const COMMITTEE: u8 = 2;
    pub const USER_POSITION: u8 = 3;
}

/// PDA seed prefixes.
pub mod seeds {
    pub const MARKET: &[u8] = b"market";
    pub const COMMITTEE: &[u8] = b"committee";
    pub const VAULT: &[u8] = b"vault";
    pub const POSITION: &[u8] = b"pos";
}
