//! # ShadowState off-chain MPC engine (Arcium-MXE model)
//!
//! This crate is the off-chain half of ShadowState. It privately aggregates user orders, matches
//! overlapping YES/NO demand peer-to-peer, computes the residual imbalance the market-maker must
//! backstop, and emits a **committee-signed batch frame** that the on-chain Pinocchio settlement
//! engine (`shadowstate-program`) verifies natively (Ed25519 precompile) and settles.
//!
//! ## Why this is a faithful model, not a stub
//!
//! Arcium's real Multi-Party eXecution Environment (MXE) runs a compiled Arcis circuit across a
//! decentralized node matrix, computing entirely over cryptographic secret shares. We cannot embed
//! a live MXE cluster in a self-contained, offline-testable crate, and the official `arcium-client`
//! SDK is disqualified here (its default transaction layer is Anchor-coupled — violating the
//! workspace's absolute No-Anchor law — and it is GPL-3.0, incompatible with this MIT crate). So we
//! implement the *primitive the MXE is built on* directly and correctly:
//!
//! - [`seal`] — clients x25519-seal their orders to the cluster key (real ECDH + ChaCha20Poly1305),
//!   exactly as an Arcium client encrypts inputs. Plaintext never travels or rests in the clear.
//! - [`secret_share`] — **additive secret sharing** over `u64` (the scheme underlying the MXE's
//!   blinded accumulator). Orders are split into shares the instant they are unsealed; every node
//!   holds only shares; accumulation is share-local. No plaintext order size or identity is visible
//!   during the computation cycle.
//! - [`mxe`] — the blinded accumulator + private P2P matching matrix. Only the *final aggregates*
//!   (which are public on-chain by design) are ever reconstructed.
//! - [`committee`] — the node matrix threshold-signs the resulting frame with Ed25519.
//! - [`frame`] — assembles the byte-identical [`protocol::BatchHeader`]`++`[`protocol::UserFill`]
//!   payload the on-chain engine parses.
//! - [`relay`] — builds the Ed25519 precompile instructions + `SubmitBatch` transaction and pushes
//!   it to the chain.
//! - [`ingestion`] — the strict 1200 ms Frequent-Batch-Auction loop tying it all together.
//!
//! The trust boundary matches the on-chain side exactly: the MPC layer owns *privacy* (it is the
//! only party that sees encrypted orders); the chain owns *deterministic pricing* and re-derives
//! every economic value from the fills, trusting only the committee signatures over the raw bytes.

pub mod committee;
pub mod engine;
pub mod error;
pub mod frame;
pub mod ingestion;
pub mod mxe;
pub mod order;
pub mod relay;
pub mod relayer;
pub mod seal;
pub mod secret_share;

pub use committee::{Committee, NodeSignature};
pub use engine::{process_epoch, EpochParams, SignedBatch};
pub use error::MpcError;
pub use mxe::{MatchResult, MxeCluster};
pub use order::{Order, Side};
pub use relayer::{ClearedBatch, RelayedBatch, Relayer, SlotOrder, SlotRegistry};
