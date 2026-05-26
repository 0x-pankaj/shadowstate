//! # ShadowState Protocol — frozen wire contract
//!
//! Shared verbatim by the on-chain Pinocchio settlement engine (`shadowstate-program`) and the
//! off-chain Arcium MPC engine (`shadowstate-mpc`). It defines:
//!
//! - economic + layout [`constants`] (fixed-point scale, price guardrails, FBA cadence),
//! - the signed batch [`frame`] (`BatchHeader` + `UserFill`),
//! - instruction / account / seed [`ids`].
//!
//! `#![no_std]` for the on-chain side; `std` is enabled under `cfg(test)` so the unit tests can
//! use `Vec`. Depending only on `bytemuck`, it is cheap for the off-chain crates to pull in.

#![cfg_attr(not(test), no_std)]

pub mod constants;
pub mod frame;
pub mod ids;

pub use constants::*;
pub use frame::{
    frame_len, read_fill, read_header, validate_frame_len, BatchHeader, UserFill, FILL_LEN,
    HEADER_LEN,
};
