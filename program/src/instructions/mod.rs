//! Instruction handlers. Each module exposes `process(accounts, data)` and is dispatched from the
//! entrypoint by the 1-byte discriminator (see `protocol::ids::ix`).

pub mod claim_mm_winnings;
pub mod claim_winnings;
pub mod close_market;
pub mod deposit_collateral;
pub mod deposit_mm_collateral;
pub mod init_user_position;
pub mod initialize_market;
pub mod resolve_market;
pub mod settle;
pub mod submit_batch;
pub mod submit_batch_trusted;
pub mod update_risk_params;
pub mod withdraw_collateral;
pub mod withdraw_mm_collateral;
