//! # ShadowState confidential matching circuits (Arcis 0.10.4)
//!
//! Compiled by `arcium build` and executed across an Arcium **Cerberus** MPC cluster — no single
//! node ever sees a plaintext order. Privacy model: *confidential order flow, positions public*.
//! Orders accumulate into an MXE-encrypted batch book; `clear_batch` reveals only the cleared result.
//!
//! arcis note: constants live **inside** the `#[encrypted]` module (the arcis compiler does not
//! resolve `use super::`). Direction tags + `BATCH_CAP` mirror the `protocol` crate.

use arcis::*;

#[encrypted]
mod circuits {
    use arcis::*;

    /// Fixed per-batch slot capacity (one public slot per order). Mirrors `protocol`/gateway.
    const BATCH_CAP: usize = 8;
    /// Residual imbalance on the YES side (MM is the forced NO counterparty).
    const DIRECTION_YES_HEAVY: u8 = 0;
    /// Residual imbalance on the NO side.
    const DIRECTION_NO_HEAVY: u8 = 1;

    /// A single client's order. `side`: 0 = YES, 1 = NO. Identity is public (carried on-chain by the
    /// slot→owner map); the secret fields are **side and size**.
    pub struct OrderInput {
        pub side: u8,
        pub qty: u64,
    }

    /// The confidential batch book — MXE-only encrypted state. Holds running totals + per-slot orders.
    pub struct BatchBook {
        pub count: u64,
        pub total_yes: u64,
        pub total_no: u64,
        pub sides: [u8; BATCH_CAP],
        pub qtys: [u64; BATCH_CAP],
    }

    /// The plaintext clearing revealed by `clear_batch`. Maps onto `protocol::BatchHeader` + the
    /// per-fill split the relayer computes from `sides`/`qtys`.
    pub struct BatchClearing {
        pub count: u64,
        pub total_yes: u64,
        pub total_no: u64,
        pub matched: u64,
        pub net_imbalance: u64,
        pub direction: u8,
        pub sides: [u8; BATCH_CAP],
        pub qtys: [u64; BATCH_CAP],
    }

    /// One-time: an empty, MXE-encrypted batch book for a market epoch.
    #[instruction]
    pub fn init_book(mxe: Mxe) -> Enc<Mxe, BatchBook> {
        let book = BatchBook {
            count: 0,
            total_yes: 0,
            total_no: 0,
            sides: [0u8; BATCH_CAP],
            qtys: [0u64; BATCH_CAP],
        };
        mxe.from_arcis(book)
    }

    /// Fold one sealed order into the encrypted book. Reveals nothing. `slot` is the plaintext public
    /// index the gateway assigned (identity public, side/size secret).
    #[instruction]
    pub fn ingest_order(
        slot: u64,
        order: Enc<Shared, OrderInput>,
        book_enc: Enc<Mxe, BatchBook>,
    ) -> Enc<Mxe, BatchBook> {
        let o = order.to_arcis();
        let mut book = book_enc.to_arcis();

        let is_yes = o.side == 0;
        book.total_yes += if is_yes { o.qty } else { 0 };
        book.total_no += if is_yes { 0 } else { o.qty };

        for i in 0..BATCH_CAP {
            let hit = (i as u64) == slot;
            book.sides[i] = if hit { o.side } else { book.sides[i] };
            book.qtys[i] = if hit { o.qty } else { book.qtys[i] };
        }
        book.count += 1;

        book_enc.owner.from_arcis(book)
    }

    /// Close the auction: compute aggregate clearing over the secret book and reveal it + the
    /// per-slot orders. No division in-circuit (the pro-rata split is done by the relayer).
    #[instruction]
    pub fn clear_batch(book_enc: Enc<Mxe, BatchBook>) -> BatchClearing {
        let book = book_enc.to_arcis();

        let yes = book.total_yes;
        let no = book.total_no;

        let yes_heavy = yes >= no;
        let matched = if yes_heavy { no } else { yes };
        let net_imbalance = if yes_heavy { yes - no } else { no - yes };
        let direction = if yes_heavy {
            DIRECTION_YES_HEAVY
        } else {
            DIRECTION_NO_HEAVY
        };

        BatchClearing {
            count: book.count,
            total_yes: yes,
            total_no: no,
            matched,
            net_imbalance,
            direction,
            sides: book.sides,
            qtys: book.qtys,
        }
        .reveal()
    }
}
