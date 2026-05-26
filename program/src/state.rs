//! Zero-copy on-chain account state. Every state account is a `#[repr(C)]` `bytemuck::Pod`
//! struct cast directly out of the (8-byte-aligned) account data buffer — no Borsh, no heap.
//!
//! Layout convention: the account discriminator lives at byte 0 and the version at byte 1, both
//! *inside* the struct, so the whole buffer (offset 0, which the SVM loader guarantees is
//! 8-aligned) is cast as the struct. Explicit `_pad*` / `_reserved` fields realign the `u64`
//! block and forbid implicit padding, which `assert_no_padding!` enforces at compile time.

use {
    crate::error::ShadowError,
    bytemuck::{Pod, Zeroable},
    pinocchio::{account::AccountView, error::ProgramError},
    protocol::{ids::account, MAX_COMMITTEE},
};

/// Compile-time guarantee that a `Pod` struct has exactly `$expected` bytes (no implicit padding).
macro_rules! assert_no_padding {
    ($t:ty, $expected:expr) => {
        const _: () = assert!(
            core::mem::size_of::<$t>() == $expected,
            concat!(stringify!($t), " has unexpected size/padding"),
        );
    };
}

/// Trait tying a `Pod` state struct to its 1-byte discriminator and on-chain length.
pub trait AccountState: Pod {
    const DISC: u8;
    const LEN: usize = core::mem::size_of::<Self>();
}

/// Per-market configuration + running aggregates. PDA: `[b"market", authority]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MarketState {
    pub disc: u8,
    pub version: u8,
    pub bump: u8,
    pub vault_bump: u8,
    /// Resolution state: `OUTCOME_UNRESOLVED` (0) / `OUTCOME_YES_WON` / `OUTCOME_NO_WON` / `OUTCOME_INVALID`.
    pub outcome: u8,
    /// Lifecycle state: `STATUS_TRADING` (0, default) / `STATUS_CLOSED`. Settlement requires trading;
    /// resolution requires closed.
    pub status: u8,
    pub _pad0: [u8; 2],
    /// MM admin authority; gates `UpdateRiskParams`.
    pub authority: [u8; 32],
    /// Token-2022 collateral mint.
    pub collateral_mint: [u8; 32],
    /// Token-2022 vault token account (authority = `[b"vault", market]` PDA).
    pub vault: [u8; 32],
    /// MM's Token-2022 account that receives the Tier-2 skew premium (spread fee) at settlement.
    pub mm_account: [u8; 32],
    /// Tier-2 PropAMM anchor price (6-dec fixed point); MM fair value.
    pub base_oracle_price: u64,
    /// Maximum premium (6-dec) added at full skew.
    pub max_skew_premium: u64,
    /// Net imbalance (contract units) at which `skew_ratio` saturates to `SCALE_FACTOR`.
    pub imbalance_threshold: u64,
    pub total_yes_supply: u64,
    pub total_no_supply: u64,
    /// Market-maker backstop YES position (contracts the MM was forced to take).
    pub mm_yes: u64,
    /// Market-maker backstop NO position.
    pub mm_no: u64,
    /// Highest settled FBA epoch; `SubmitBatch` requires `header.epoch > last_epoch`.
    pub last_epoch: u64,
    /// MM backstop collateral posted into the vault and not yet reserved by a settled batch. Each
    /// Tier-2 residual reserves `(SCALE − heavy_price)·net` here so the vault fully collateralizes
    /// every contract to `$1` for winner payout. Withdrawable only down to the reserved floor.
    pub mm_collateral: u64,
    /// Trusted settlement authority for the *gateway* path (`SubmitBatchTrusted`). When non-zero, a
    /// transaction signed by this key may settle a batch **without** committee signatures — the trust
    /// anchor is then the Arcium MXE attestation (verified in the off-chain `arcium-gateway`), not our
    /// native committee. All-zero ⇒ the trusted path is disabled and only committee settlement works.
    pub settlement_authority: [u8; 32],
}
assert_no_padding!(MarketState, 240);

impl MarketState {
    /// True when a trusted gateway settlement authority is configured.
    #[inline]
    pub fn trusted_settlement_enabled(&self) -> bool {
        self.settlement_authority != [0u8; 32]
    }

    /// True while trading is open (batches may settle).
    #[inline]
    pub fn is_trading(&self) -> bool {
        self.status == protocol::STATUS_TRADING
    }

    /// True once trading is closed (resolution permitted).
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.status == protocol::STATUS_CLOSED
    }
}

impl AccountState for MarketState {
    const DISC: u8 = account::MARKET_STATE;
}

/// Immutable set of trusted MPC node Ed25519 addresses. PDA: `[b"committee", market]`.
/// Written once at `InitializeMarket`; the program exposes no instruction that mutates it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Committee {
    pub disc: u8,
    pub version: u8,
    /// Number of populated entries in `members` (`1..=MAX_COMMITTEE`).
    pub count: u8,
    /// Minimum distinct member signatures required to settle a batch (`1..=count`).
    pub threshold: u8,
    pub _pad0: [u8; 4],
    pub members: [[u8; 32]; MAX_COMMITTEE],
}
assert_no_padding!(Committee, 8 + 32 * MAX_COMMITTEE);

impl AccountState for Committee {
    const DISC: u8 = account::COMMITTEE;
}

impl Committee {
    /// Returns `true` if `key` is one of the first `count` committee members.
    #[inline]
    pub fn contains(&self, key: &[u8; 32]) -> bool {
        let n = self.count as usize;
        // Bounded loop; `count` is validated `<= MAX_COMMITTEE` at init.
        self.members.iter().take(n).any(|m| m == key)
    }

    /// Index of `key` among the first `count` members, if present.
    #[inline]
    pub fn index_of(&self, key: &[u8; 32]) -> Option<usize> {
        let n = self.count as usize;
        self.members.iter().take(n).position(|m| m == key)
    }
}

/// Per-user position + collateral ledger. PDA: `[b"pos", market, owner]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct UserPosition {
    pub disc: u8,
    pub version: u8,
    pub bump: u8,
    pub _pad0: [u8; 5],
    pub owner: [u8; 32],
    pub market: [u8; 32],
    pub yes_qty: u64,
    pub no_qty: u64,
    /// Deposited collateral in mint base units; debited/credited at settlement.
    pub collateral: u64,
    pub _reserved: [u8; 8],
}
assert_no_padding!(UserPosition, 104);

impl AccountState for UserPosition {
    const DISC: u8 = account::USER_POSITION;
}

/// Zero-copy immutable view of an initialized state account's data buffer.
///
/// Validates length, discriminator and (via `bytemuck`) alignment. Caller is responsible for
/// checking the account is owned by this program before trusting the contents.
#[inline]
pub fn cast<T: AccountState>(data: &[u8]) -> Result<&T, ProgramError> {
    let slice = data.get(..T::LEN).ok_or(ShadowError::InvalidAccountData)?;
    if slice[0] != T::DISC {
        return Err(ShadowError::InvalidAccountData.into());
    }
    bytemuck::try_from_bytes(slice).map_err(|_| ShadowError::InvalidAccountData.into())
}

/// Zero-copy mutable view of an initialized state account's data buffer.
#[inline]
pub fn cast_mut<T: AccountState>(data: &mut [u8]) -> Result<&mut T, ProgramError> {
    let slice = data.get_mut(..T::LEN).ok_or(ShadowError::InvalidAccountData)?;
    if slice[0] != T::DISC {
        return Err(ShadowError::InvalidAccountData.into());
    }
    bytemuck::try_from_bytes_mut(slice).map_err(|_| ShadowError::InvalidAccountData.into())
}

/// Zero-copy mutable view of a freshly created (zeroed) account buffer, for *initialization*.
/// Unlike [`cast_mut`] it does not require the discriminator to be set yet — the caller writes
/// `disc`/`version` after casting. Requires the buffer currently be the uninitialized sentinel
/// to prevent re-initializing a live account.
#[inline]
pub fn cast_uninit_mut<T: AccountState>(data: &mut [u8]) -> Result<&mut T, ProgramError> {
    let slice = data.get_mut(..T::LEN).ok_or(ShadowError::InvalidAccountData)?;
    if slice[0] != account::UNINITIALIZED {
        // Already initialized — refuse to clobber it.
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    bytemuck::try_from_bytes_mut(slice).map_err(|_| ShadowError::InvalidAccountData.into())
}

/// Asserts an account is owned by this program (state accounts only).
#[inline]
pub fn require_owned_by_program(account: &AccountView) -> Result<(), ProgramError> {
    if account.owned_by(&crate::ID) {
        Ok(())
    } else {
        Err(ShadowError::InvalidAccount.into())
    }
}
