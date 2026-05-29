//! ShadowState Arcium MXE gateway (Anchor + Arcium 0.10.4).
//!
//! The thin Anchor program that owns the confidential batch book and the three-instruction pattern
//! (init_comp_def → queue → callback) for the `encrypted-ixs` circuits: `init_book`, `ingest_order`,
//! `clear_batch`. It holds no settlement logic — on `clear_batch` it emits `BatchCleared`, which the
//! off-chain relayer turns into a `SubmitBatchTrusted` on the pure-Pinocchio settlement program.

use anchor_lang::prelude::*;
use arcium_anchor::prelude::*;
use arcium_client::idl::arcium::types::CallbackAccount;

const COMP_DEF_OFFSET_INIT_BOOK: u32 = comp_def_offset("init_book");
const COMP_DEF_OFFSET_INGEST_ORDER: u32 = comp_def_offset("ingest_order");
const COMP_DEF_OFFSET_CLEAR_BATCH: u32 = comp_def_offset("clear_batch");

declare_id!("E3GFUytcsMFgYgwTrHoob1YvhB4UvqTzj4bFWzE5dNXe");

/// `BatchBook` ciphertext field count: count, total_yes, total_no, sides[8], qtys[8].
const BATCH_BOOK_FIELDS: usize = 19;
/// Byte offset of `book_state` in `BatchBookAccount`: 8 disc + 1 bump + 32 authority + 32 market + 8 epoch + 1 slot_count.
const BOOK_STATE_OFFSET: u32 = 8 + 1 + 32 + 32 + 8 + 1;
/// `book_state` byte size: 19 ciphertexts × 32.
const BOOK_STATE_SIZE: u32 = (BATCH_BOOK_FIELDS as u32) * 32;

#[arcium_program]
pub mod shadowstate_mxe {
    use super::*;

    // ============================== init_book ==================================================

    pub fn init_init_book_comp_def(ctx: Context<InitInitBookCompDef>) -> Result<()> {
        init_computation_def(ctx.accounts, None)?;
        Ok(())
    }

    pub fn init_book(
        ctx: Context<InitBook>,
        computation_offset: u64,
        market: Pubkey,
        epoch: u64,
    ) -> Result<()> {
        ctx.accounts.sign_pda_account.bump = ctx.bumps.sign_pda_account;

        let book = &mut ctx.accounts.book;
        book.bump = ctx.bumps.book;
        book.authority = ctx.accounts.payer.key();
        book.market = market;
        book.epoch = epoch;
        book.slot_count = 0;
        book.nonce = 0;

        let args = ArgBuilder::new().build();

        queue_computation(
            ctx.accounts,
            computation_offset,
            args,
            vec![InitBookCallback::callback_ix(
                computation_offset,
                &ctx.accounts.mxe_account,
                &[CallbackAccount {
                    pubkey: ctx.accounts.book.key(),
                    is_writable: true,
                }],
            )?],
            1,
            0,
        )?;
        Ok(())
    }

    #[arcium_callback(encrypted_ix = "init_book")]
    pub fn init_book_callback(
        ctx: Context<InitBookCallback>,
        output: SignedComputationOutputs<InitBookOutput>,
    ) -> Result<()> {
        let o = match output
            .verify_output(&ctx.accounts.cluster_account, &ctx.accounts.computation_account)
        {
            Ok(InitBookOutput { field_0 }) => field_0,
            Err(_) => return Err(ErrorCode::AbortedComputation.into()),
        };
        let book = &mut ctx.accounts.book;
        for i in 0..BATCH_BOOK_FIELDS {
            book.book_state[i] = o.ciphertexts[i];
        }
        book.nonce = o.nonce;
        emit!(BookOpened {
            book: book.key(),
            market: book.market,
            epoch: book.epoch,
        });
        Ok(())
    }

    // ============================== ingest_order ==============================================

    pub fn init_ingest_order_comp_def(ctx: Context<InitIngestOrderCompDef>) -> Result<()> {
        init_computation_def(ctx.accounts, None)?;
        Ok(())
    }

    pub fn ingest_order(
        ctx: Context<IngestOrder>,
        computation_offset: u64,
        order_side_ct: [u8; 32],
        order_qty_ct: [u8; 32],
        enc_pubkey: [u8; 32],
        order_nonce: u128,
    ) -> Result<()> {
        ctx.accounts.sign_pda_account.bump = ctx.bumps.sign_pda_account;

        let book = &mut ctx.accounts.book;
        require!((book.slot_count as usize) < BATCH_BOOK_FIELDS, ErrorCode::BookFull);
        let slot = book.slot_count as u64;
        book.slot_count += 1;
        let book_nonce = book.nonce;
        let book_key = book.key();

        let args = ArgBuilder::new()
            .plaintext_u64(slot)
            .x25519_pubkey(enc_pubkey)
            .plaintext_u128(order_nonce)
            .encrypted_u8(order_side_ct)
            .encrypted_u64(order_qty_ct)
            .plaintext_u128(book_nonce)
            .account(book_key, BOOK_STATE_OFFSET, BOOK_STATE_SIZE)
            .build();

        queue_computation(
            ctx.accounts,
            computation_offset,
            args,
            vec![IngestOrderCallback::callback_ix(
                computation_offset,
                &ctx.accounts.mxe_account,
                &[CallbackAccount {
                    pubkey: book_key,
                    is_writable: true,
                }],
            )?],
            1,
            0,
        )?;
        Ok(())
    }

    #[arcium_callback(encrypted_ix = "ingest_order")]
    pub fn ingest_order_callback(
        ctx: Context<IngestOrderCallback>,
        output: SignedComputationOutputs<IngestOrderOutput>,
    ) -> Result<()> {
        let o = match output
            .verify_output(&ctx.accounts.cluster_account, &ctx.accounts.computation_account)
        {
            Ok(IngestOrderOutput { field_0 }) => field_0,
            Err(_) => return Err(ErrorCode::AbortedComputation.into()),
        };
        let book = &mut ctx.accounts.book;
        for i in 0..BATCH_BOOK_FIELDS {
            book.book_state[i] = o.ciphertexts[i];
        }
        book.nonce = o.nonce;
        emit!(OrderIngested {
            book: book.key(),
            market: book.market,
            epoch: book.epoch,
            slot_count: book.slot_count,
        });
        Ok(())
    }

    // ============================== clear_batch ===============================================

    pub fn init_clear_batch_comp_def(ctx: Context<InitClearBatchCompDef>) -> Result<()> {
        init_computation_def(ctx.accounts, None)?;
        Ok(())
    }

    pub fn clear_batch(ctx: Context<ClearBatch>, computation_offset: u64) -> Result<()> {
        ctx.accounts.sign_pda_account.bump = ctx.bumps.sign_pda_account;
        require_keys_eq!(ctx.accounts.book.authority, ctx.accounts.payer.key(), ErrorCode::Unauthorized);

        let book_nonce = ctx.accounts.book.nonce;
        let book_key = ctx.accounts.book.key();

        let args = ArgBuilder::new()
            .plaintext_u128(book_nonce)
            .account(book_key, BOOK_STATE_OFFSET, BOOK_STATE_SIZE)
            .build();

        queue_computation(
            ctx.accounts,
            computation_offset,
            args,
            vec![ClearBatchCallback::callback_ix(
                computation_offset,
                &ctx.accounts.mxe_account,
                &[CallbackAccount {
                    pubkey: book_key,
                    is_writable: false,
                }],
            )?],
            1,
            0,
        )?;
        Ok(())
    }

    #[arcium_callback(encrypted_ix = "clear_batch")]
    pub fn clear_batch_callback(
        ctx: Context<ClearBatchCallback>,
        output: SignedComputationOutputs<ClearBatchOutput>,
    ) -> Result<()> {
        let c = match output
            .verify_output(&ctx.accounts.cluster_account, &ctx.accounts.computation_account)
        {
            Ok(ClearBatchOutput { field_0 }) => field_0,
            Err(_) => return Err(ErrorCode::AbortedComputation.into()),
        };
        let book = &ctx.accounts.book;
        // The revealed `BatchClearing` output is positional: field_0..field_7 = count, total_yes,
        // total_no, matched, net_imbalance, direction, sides, qtys.
        emit!(BatchCleared {
            book: book.key(),
            market: book.market,
            epoch: book.epoch,
            count: c.field_0,
            total_yes: c.field_1,
            total_no: c.field_2,
            matched: c.field_3,
            net_imbalance: c.field_4,
            direction: c.field_5,
            sides: c.field_6,
            qtys: c.field_7,
        });
        Ok(())
    }
}

// ================================= state ========================================================

#[account]
#[derive(InitSpace)]
pub struct BatchBookAccount {
    pub bump: u8,
    pub authority: Pubkey,
    pub market: Pubkey,
    pub epoch: u64,
    pub slot_count: u8,
    pub book_state: [[u8; 32]; BATCH_BOOK_FIELDS],
    pub nonce: u128,
}

// ================================= comp-def init accounts =======================================

#[init_computation_definition_accounts("init_book", payer)]
#[derive(Accounts)]
pub struct InitInitBookCompDef<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut, address = derive_mxe_pda!())]
    pub mxe_account: Box<Account<'info, MXEAccount>>,
    #[account(mut)]
    /// CHECK: comp_def_account, checked by arcium program.
    pub comp_def_account: UncheckedAccount<'info>,
    #[account(mut, address = derive_mxe_lut_pda!(mxe_account.lut_offset_slot))]
    /// CHECK: address_lookup_table, checked by arcium program.
    pub address_lookup_table: UncheckedAccount<'info>,
    #[account(address = LUT_PROGRAM_ID)]
    /// CHECK: lut_program.
    pub lut_program: UncheckedAccount<'info>,
    pub arcium_program: Program<'info, Arcium>,
    pub system_program: Program<'info, System>,
}

#[init_computation_definition_accounts("ingest_order", payer)]
#[derive(Accounts)]
pub struct InitIngestOrderCompDef<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut, address = derive_mxe_pda!())]
    pub mxe_account: Box<Account<'info, MXEAccount>>,
    #[account(mut)]
    /// CHECK: comp_def_account, checked by arcium program.
    pub comp_def_account: UncheckedAccount<'info>,
    #[account(mut, address = derive_mxe_lut_pda!(mxe_account.lut_offset_slot))]
    /// CHECK: address_lookup_table.
    pub address_lookup_table: UncheckedAccount<'info>,
    #[account(address = LUT_PROGRAM_ID)]
    /// CHECK: lut_program.
    pub lut_program: UncheckedAccount<'info>,
    pub arcium_program: Program<'info, Arcium>,
    pub system_program: Program<'info, System>,
}

#[init_computation_definition_accounts("clear_batch", payer)]
#[derive(Accounts)]
pub struct InitClearBatchCompDef<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut, address = derive_mxe_pda!())]
    pub mxe_account: Box<Account<'info, MXEAccount>>,
    #[account(mut)]
    /// CHECK: comp_def_account, checked by arcium program.
    pub comp_def_account: UncheckedAccount<'info>,
    #[account(mut, address = derive_mxe_lut_pda!(mxe_account.lut_offset_slot))]
    /// CHECK: address_lookup_table.
    pub address_lookup_table: UncheckedAccount<'info>,
    #[account(address = LUT_PROGRAM_ID)]
    /// CHECK: lut_program.
    pub lut_program: UncheckedAccount<'info>,
    pub arcium_program: Program<'info, Arcium>,
    pub system_program: Program<'info, System>,
}

// ================================= queue accounts ===============================================

#[queue_computation_accounts("init_book", payer)]
#[derive(Accounts)]
#[instruction(computation_offset: u64, market: Pubkey, epoch: u64)]
pub struct InitBook<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(init_if_needed, space = 9, payer = payer, seeds = [&SIGN_PDA_SEED], bump, address = derive_sign_pda!())]
    pub sign_pda_account: Account<'info, ArciumSignerAccount>,
    #[account(address = derive_mxe_pda!())]
    pub mxe_account: Box<Account<'info, MXEAccount>>,
    #[account(mut, address = derive_mempool_pda!(mxe_account))]
    /// CHECK: mempool_account.
    pub mempool_account: UncheckedAccount<'info>,
    #[account(mut, address = derive_execpool_pda!(mxe_account))]
    /// CHECK: executing_pool.
    pub executing_pool: UncheckedAccount<'info>,
    #[account(mut, address = derive_comp_pda!(computation_offset, mxe_account))]
    /// CHECK: computation_account.
    pub computation_account: UncheckedAccount<'info>,
    #[account(address = derive_comp_def_pda!(COMP_DEF_OFFSET_INIT_BOOK))]
    pub comp_def_account: Box<Account<'info, ComputationDefinitionAccount>>,
    #[account(mut, address = derive_cluster_pda!(mxe_account))]
    pub cluster_account: Box<Account<'info, Cluster>>,
    #[account(mut, address = ARCIUM_FEE_POOL_ACCOUNT_ADDRESS)]
    pub pool_account: Account<'info, FeePool>,
    #[account(mut, address = ARCIUM_CLOCK_ACCOUNT_ADDRESS)]
    pub clock_account: Account<'info, ClockAccount>,
    pub system_program: Program<'info, System>,
    pub arcium_program: Program<'info, Arcium>,
    #[account(
        init_if_needed, payer = payer, space = 8 + BatchBookAccount::INIT_SPACE,
        seeds = [b"book", market.as_ref(), &epoch.to_le_bytes()], bump,
    )]
    pub book: Box<Account<'info, BatchBookAccount>>,
}

#[queue_computation_accounts("ingest_order", payer)]
#[derive(Accounts)]
#[instruction(computation_offset: u64)]
pub struct IngestOrder<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(init_if_needed, space = 9, payer = payer, seeds = [&SIGN_PDA_SEED], bump, address = derive_sign_pda!())]
    pub sign_pda_account: Account<'info, ArciumSignerAccount>,
    #[account(address = derive_mxe_pda!())]
    pub mxe_account: Box<Account<'info, MXEAccount>>,
    #[account(mut, address = derive_mempool_pda!(mxe_account))]
    /// CHECK: mempool_account.
    pub mempool_account: UncheckedAccount<'info>,
    #[account(mut, address = derive_execpool_pda!(mxe_account))]
    /// CHECK: executing_pool.
    pub executing_pool: UncheckedAccount<'info>,
    #[account(mut, address = derive_comp_pda!(computation_offset, mxe_account))]
    /// CHECK: computation_account.
    pub computation_account: UncheckedAccount<'info>,
    #[account(address = derive_comp_def_pda!(COMP_DEF_OFFSET_INGEST_ORDER))]
    pub comp_def_account: Box<Account<'info, ComputationDefinitionAccount>>,
    #[account(mut, address = derive_cluster_pda!(mxe_account))]
    pub cluster_account: Box<Account<'info, Cluster>>,
    #[account(mut, address = ARCIUM_FEE_POOL_ACCOUNT_ADDRESS)]
    pub pool_account: Account<'info, FeePool>,
    #[account(mut, address = ARCIUM_CLOCK_ACCOUNT_ADDRESS)]
    pub clock_account: Account<'info, ClockAccount>,
    pub system_program: Program<'info, System>,
    pub arcium_program: Program<'info, Arcium>,
    #[account(mut, seeds = [b"book", book.market.as_ref(), &book.epoch.to_le_bytes()], bump = book.bump)]
    pub book: Box<Account<'info, BatchBookAccount>>,
}

#[queue_computation_accounts("clear_batch", payer)]
#[derive(Accounts)]
#[instruction(computation_offset: u64)]
pub struct ClearBatch<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(init_if_needed, space = 9, payer = payer, seeds = [&SIGN_PDA_SEED], bump, address = derive_sign_pda!())]
    pub sign_pda_account: Account<'info, ArciumSignerAccount>,
    #[account(address = derive_mxe_pda!())]
    pub mxe_account: Box<Account<'info, MXEAccount>>,
    #[account(mut, address = derive_mempool_pda!(mxe_account))]
    /// CHECK: mempool_account.
    pub mempool_account: UncheckedAccount<'info>,
    #[account(mut, address = derive_execpool_pda!(mxe_account))]
    /// CHECK: executing_pool.
    pub executing_pool: UncheckedAccount<'info>,
    #[account(mut, address = derive_comp_pda!(computation_offset, mxe_account))]
    /// CHECK: computation_account.
    pub computation_account: UncheckedAccount<'info>,
    #[account(address = derive_comp_def_pda!(COMP_DEF_OFFSET_CLEAR_BATCH))]
    pub comp_def_account: Box<Account<'info, ComputationDefinitionAccount>>,
    #[account(mut, address = derive_cluster_pda!(mxe_account))]
    pub cluster_account: Box<Account<'info, Cluster>>,
    #[account(mut, address = ARCIUM_FEE_POOL_ACCOUNT_ADDRESS)]
    pub pool_account: Account<'info, FeePool>,
    #[account(mut, address = ARCIUM_CLOCK_ACCOUNT_ADDRESS)]
    pub clock_account: Account<'info, ClockAccount>,
    pub system_program: Program<'info, System>,
    pub arcium_program: Program<'info, Arcium>,
    #[account(seeds = [b"book", book.market.as_ref(), &book.epoch.to_le_bytes()], bump = book.bump)]
    pub book: Box<Account<'info, BatchBookAccount>>,
}

// ================================= callback accounts ============================================

#[callback_accounts("init_book")]
#[derive(Accounts)]
pub struct InitBookCallback<'info> {
    pub arcium_program: Program<'info, Arcium>,
    #[account(address = derive_comp_def_pda!(COMP_DEF_OFFSET_INIT_BOOK))]
    pub comp_def_account: Account<'info, ComputationDefinitionAccount>,
    #[account(address = derive_mxe_pda!())]
    pub mxe_account: Account<'info, MXEAccount>,
    /// CHECK: computation_account, checked by arcium program.
    pub computation_account: UncheckedAccount<'info>,
    #[account(address = derive_cluster_pda!(mxe_account))]
    pub cluster_account: Account<'info, Cluster>,
    #[account(address = ::arcium_anchor::solana_instructions_sysvar::ID)]
    /// CHECK: instructions_sysvar.
    pub instructions_sysvar: UncheckedAccount<'info>,
    #[account(mut)]
    pub book: Box<Account<'info, BatchBookAccount>>,
}

#[callback_accounts("ingest_order")]
#[derive(Accounts)]
pub struct IngestOrderCallback<'info> {
    pub arcium_program: Program<'info, Arcium>,
    #[account(address = derive_comp_def_pda!(COMP_DEF_OFFSET_INGEST_ORDER))]
    pub comp_def_account: Account<'info, ComputationDefinitionAccount>,
    #[account(address = derive_mxe_pda!())]
    pub mxe_account: Account<'info, MXEAccount>,
    /// CHECK: computation_account.
    pub computation_account: UncheckedAccount<'info>,
    #[account(address = derive_cluster_pda!(mxe_account))]
    pub cluster_account: Account<'info, Cluster>,
    #[account(address = ::arcium_anchor::solana_instructions_sysvar::ID)]
    /// CHECK: instructions_sysvar.
    pub instructions_sysvar: UncheckedAccount<'info>,
    #[account(mut)]
    pub book: Box<Account<'info, BatchBookAccount>>,
}

#[callback_accounts("clear_batch")]
#[derive(Accounts)]
pub struct ClearBatchCallback<'info> {
    pub arcium_program: Program<'info, Arcium>,
    #[account(address = derive_comp_def_pda!(COMP_DEF_OFFSET_CLEAR_BATCH))]
    pub comp_def_account: Account<'info, ComputationDefinitionAccount>,
    #[account(address = derive_mxe_pda!())]
    pub mxe_account: Account<'info, MXEAccount>,
    /// CHECK: computation_account.
    pub computation_account: UncheckedAccount<'info>,
    #[account(address = derive_cluster_pda!(mxe_account))]
    pub cluster_account: Account<'info, Cluster>,
    #[account(address = ::arcium_anchor::solana_instructions_sysvar::ID)]
    /// CHECK: instructions_sysvar.
    pub instructions_sysvar: UncheckedAccount<'info>,
    pub book: Box<Account<'info, BatchBookAccount>>,
}

// ================================= events + errors =============================================

#[event]
pub struct BookOpened {
    pub book: Pubkey,
    pub market: Pubkey,
    pub epoch: u64,
}

#[event]
pub struct OrderIngested {
    pub book: Pubkey,
    pub market: Pubkey,
    pub epoch: u64,
    pub slot_count: u8,
}

#[event]
pub struct BatchCleared {
    pub book: Pubkey,
    pub market: Pubkey,
    pub epoch: u64,
    pub count: u64,
    pub total_yes: u64,
    pub total_no: u64,
    pub matched: u64,
    pub net_imbalance: u64,
    pub direction: u8,
    pub sides: [u8; 8],
    pub qtys: [u64; 8],
}

#[error_code]
pub enum ErrorCode {
    #[msg("The computation was aborted")]
    AbortedComputation,
    #[msg("Batch book is full")]
    BookFull,
    #[msg("Unauthorized")]
    Unauthorized,
}
