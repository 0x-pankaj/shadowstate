//! End-to-end LiteSVM tests for the ShadowState settlement engine.
//!
//! Each test boots a LiteSVM with the real bundled Token-2022 program + the Ed25519 precompile,
//! loads the compiled `shadowstate_program.so`, and drives the program through real transactions:
//! market init, position init, collateral deposit, committee-signed batch settlement, replay,
//! signature-security negatives, risk-param update, and withdrawal.
//!
//! Build the SBF artifact first: `cargo build-sbf --manifest-path program/Cargo.toml --tools-version v1.52`.

use {
    bytemuck::bytes_of,
    ed25519_dalek::{Signer as _, SigningKey},
    litesvm::LiteSVM,
    protocol::{
        frame::{BatchHeader, UserFill},
        ids::ix,
        DIRECTION_YES_HEAVY, OUTCOME_INVALID, OUTCOME_NO_WON, OUTCOME_YES_WON, STATUS_CLOSED,
    },
    shadowstate_program::state::{AccountState, MarketState, UserPosition},
    solana_account::Account,
    solana_address::Address,
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

// ---- well-known ids --------------------------------------------------------------------------

const PROGRAM_ID: Address = Address::new_from_array([
    0x53, 0x68, 0x61, 0x64, 0x6f, 0x77, 0x53, 0x74, 0x61, 0x74, 0x65, 0x31, 0x31, 0x31, 0x31, 0x31,
    0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31, 0x31,
]);
const TOKEN_2022: Address = Address::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const ED25519: Address = Address::from_str_const("Ed25519SigVerify111111111111111111111111111");
const SYSTEM: Address = Address::from_str_const("11111111111111111111111111111111");
const IX_SYSVAR: Address = Address::from_str_const("Sysvar1nstructions1111111111111111111111111");

const MINT_LEN: usize = 82;
const TOKEN_ACCT_LEN: usize = 165;
const DECIMALS: u8 = 6;

// Risk params used across tests.
const BASE_PRICE: u64 = 500_000; // $0.50
const MAX_PREMIUM: u64 = 100_000; // $0.10
const THRESHOLD_IMBALANCE: u64 = 1_000;

// ---- harness ---------------------------------------------------------------------------------

struct Env {
    svm: LiteSVM,
    payer: Keypair,
    mint: Keypair,
    mint_authority: Keypair,
}

impl Env {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let so = include_bytes!("../../target/deploy/shadowstate_program.so");
        svm.add_program(PROGRAM_ID, so).unwrap();

        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 1_000_000_000_000).unwrap();

        let mint = Keypair::new();
        let mint_authority = Keypair::new();
        let mut env = Env {
            svm,
            payer,
            mint,
            mint_authority,
        };
        env.create_mint();
        env
    }

    fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> litesvm::types::TransactionResult {
        let bh = self.svm.latest_blockhash();
        let mut all: Vec<&Keypair> = vec![&self.payer];
        all.extend_from_slice(signers);
        let tx = Transaction::new_signed_with_payer(ixs, Some(&self.payer.pubkey()), &all, bh);
        self.svm.send_transaction(tx)
    }

    fn rent(&self, len: usize) -> u64 {
        self.svm.minimum_balance_for_rent_exemption(len)
    }

    fn put_owned_zeroed(&mut self, addr: Address, len: usize, owner: Address) {
        let acct = Account {
            lamports: self.rent(len),
            data: vec![0u8; len],
            owner,
            executable: false,
            rent_epoch: 0,
        };
        self.svm.set_account(addr, acct).unwrap();
    }

    fn create_mint(&mut self) {
        let mint = self.mint.pubkey();
        self.put_owned_zeroed(mint, MINT_LEN, TOKEN_2022);
        // InitializeMint2 (tag 20): decimals, mint_authority(32), freeze COption None (0).
        let mut data = vec![20u8, DECIMALS];
        data.extend_from_slice(self.mint_authority.pubkey().as_array());
        data.push(0);
        let ix = Instruction::new_with_bytes(TOKEN_2022, &data, vec![AccountMeta::new(mint, false)]);
        self.send(&[ix], &[]).unwrap();
    }

    /// Create a Token-2022 token account whose token authority is `owner`.
    fn create_token_account(&mut self, owner: Address) -> Address {
        let kp = Keypair::new();
        let addr = kp.pubkey();
        self.put_owned_zeroed(addr, TOKEN_ACCT_LEN, TOKEN_2022);
        // InitializeAccount3 (tag 18): owner(32). Accounts: [account, mint].
        let mut data = vec![18u8];
        data.extend_from_slice(owner.as_array());
        let ix = Instruction::new_with_bytes(
            TOKEN_2022,
            &data,
            vec![
                AccountMeta::new(addr, false),
                AccountMeta::new_readonly(self.mint.pubkey(), false),
            ],
        );
        self.send(&[ix], &[]).unwrap();
        addr
    }

    fn mint_to(&mut self, dest: Address, amount: u64) {
        // MintTo (tag 7): amount(8). Accounts: [mint, dest, authority(signer)].
        let mut data = vec![7u8];
        data.extend_from_slice(&amount.to_le_bytes());
        let ix = Instruction::new_with_bytes(
            TOKEN_2022,
            &data,
            vec![
                AccountMeta::new(self.mint.pubkey(), false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(self.mint_authority.pubkey(), true),
            ],
        );
        let auth = self.mint_authority.insecure_clone();
        self.send(&[ix], &[&auth]).unwrap();
    }

    fn token_balance(&self, account: Address) -> u64 {
        let acct = self.svm.get_account(&account).unwrap();
        u64::from_le_bytes(acct.data[64..72].try_into().unwrap())
    }

    fn read_market(&self, market: Address) -> MarketState {
        let acct = self.svm.get_account(&market).unwrap();
        bytemuck::pod_read_unaligned(&acct.data[..MarketState::LEN])
    }

    fn read_position(&self, position: Address) -> UserPosition {
        let acct = self.svm.get_account(&position).unwrap();
        bytemuck::pod_read_unaligned(&acct.data[..UserPosition::LEN])
    }
}

// ---- PDA helpers -----------------------------------------------------------------------------

fn market_pda(authority: Address) -> (Address, u8) {
    Address::find_program_address(&[b"market", authority.as_array()], &PROGRAM_ID)
}
fn committee_pda(market: Address) -> (Address, u8) {
    Address::find_program_address(&[b"committee", market.as_array()], &PROGRAM_ID)
}
fn vault_pda(market: Address) -> (Address, u8) {
    Address::find_program_address(&[b"vault", market.as_array()], &PROGRAM_ID)
}
fn position_pda(market: Address, owner: Address) -> (Address, u8) {
    Address::find_program_address(&[b"pos", market.as_array(), owner.as_array()], &PROGRAM_ID)
}

// ---- committee + ed25519 helpers -------------------------------------------------------------

/// A deterministic committee of `n` Ed25519 signing keys (seeds 1..=n).
fn committee(n: u8) -> Vec<SigningKey> {
    (1..=n)
        .map(|i| {
            let mut seed = [0u8; 32];
            seed[0] = i;
            SigningKey::from_bytes(&seed)
        })
        .collect()
}

fn member_bytes(keys: &[SigningKey]) -> Vec<u8> {
    let mut out = Vec::new();
    for k in keys {
        out.extend_from_slice(&k.verifying_key().to_bytes());
    }
    out
}

/// Build an Ed25519 precompile instruction carrying one self-contained signature over `msg`.
/// Layout: header(16) | pubkey(32) @16 | signature(64) @48 | message @112.
fn ed25519_ix(key: &SigningKey, msg: &[u8]) -> Instruction {
    let pubkey = key.verifying_key().to_bytes();
    let sig = key.sign(msg).to_bytes();

    let pk_off: u16 = 16;
    let sig_off: u16 = 48;
    let msg_off: u16 = 112;

    let mut d = Vec::with_capacity(112 + msg.len());
    d.push(1); // num_signatures
    d.push(0); // padding
    d.extend_from_slice(&sig_off.to_le_bytes());
    d.extend_from_slice(&u16::MAX.to_le_bytes()); // signature instruction index = self
    d.extend_from_slice(&pk_off.to_le_bytes());
    d.extend_from_slice(&u16::MAX.to_le_bytes()); // pubkey instruction index = self
    d.extend_from_slice(&msg_off.to_le_bytes());
    d.extend_from_slice(&(msg.len() as u16).to_le_bytes());
    d.extend_from_slice(&u16::MAX.to_le_bytes()); // message instruction index = self
    d.extend_from_slice(&pubkey); // @16
    d.extend_from_slice(&sig); // @48
    d.extend_from_slice(msg); // @112

    Instruction::new_with_bytes(ED25519, &d, vec![])
}

fn frame_bytes(header: &BatchHeader, fills: &[UserFill]) -> Vec<u8> {
    let mut v = Vec::with_capacity(72 + fills.len() * 64);
    v.extend_from_slice(bytes_of(header));
    for f in fills {
        v.extend_from_slice(bytes_of(f));
    }
    v
}

fn header(market: Address, epoch: u64, net: u64, direction: u8, fills: &[UserFill]) -> BatchHeader {
    let p2p_volume = fills.iter().map(|f| f.p2p_yes + f.p2p_no).sum();
    BatchHeader {
        market: *market.as_array(),
        epoch,
        batch_id: epoch,
        p2p_volume,
        net_imbalance: net,
        fill_count: fills.len() as u16,
        direction,
        _pad: [0; 5],
    }
}

fn fill(user: Address, p2p_yes: u64, p2p_no: u64, residual_yes: u64, residual_no: u64) -> UserFill {
    UserFill {
        user: *user.as_array(),
        p2p_yes,
        p2p_no,
        residual_yes,
        residual_no,
    }
}

// ---- program instruction builders ------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn ix_initialize_market(
    payer: Address,
    authority: Address,
    mint: Address,
    vault: Address,
    mm_account: Address,
    market: Address,
    committee_acc: Address,
    members: &[u8],
    count: u8,
    threshold: u8,
    settlement_authority: Option<Address>,
) -> Instruction {
    let mut data = vec![ix::INITIALIZE_MARKET];
    data.extend_from_slice(&BASE_PRICE.to_le_bytes());
    data.extend_from_slice(&MAX_PREMIUM.to_le_bytes());
    data.extend_from_slice(&THRESHOLD_IMBALANCE.to_le_bytes());
    data.push(count);
    data.push(threshold);
    data.extend_from_slice(members);
    if let Some(sa) = settlement_authority {
        data.extend_from_slice(sa.as_array());
    }
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mm_account, false),
            AccountMeta::new(market, false),
            AccountMeta::new(committee_acc, false),
            AccountMeta::new_readonly(SYSTEM, false),
        ],
    )
}

fn ix_init_position(payer: Address, owner: Address, market: Address, position: Address) -> Instruction {
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &[ix::INIT_USER_POSITION],
        vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(position, false),
            AccountMeta::new_readonly(SYSTEM, false),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn ix_deposit(
    owner: Address,
    market: Address,
    position: Address,
    user_token: Address,
    vault: Address,
    mint: Address,
    amount: u64,
) -> Instruction {
    let mut data = vec![ix::DEPOSIT_COLLATERAL];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(position, false),
            AccountMeta::new(user_token, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(TOKEN_2022, false),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn ix_submit_batch(
    relayer: Address,
    market: Address,
    committee_acc: Address,
    vault: Address,
    mm_account: Address,
    mint: Address,
    vault_authority: Address,
    positions: &[Address],
    frame: &[u8],
) -> Instruction {
    let mut data = vec![ix::SUBMIT_BATCH];
    data.extend_from_slice(frame);
    let mut metas = vec![
        AccountMeta::new_readonly(relayer, true),
        AccountMeta::new(market, false),
        AccountMeta::new_readonly(committee_acc, false),
        AccountMeta::new_readonly(IX_SYSVAR, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(mm_account, false),
        AccountMeta::new_readonly(mint, false),
        AccountMeta::new_readonly(vault_authority, false),
        AccountMeta::new_readonly(TOKEN_2022, false),
    ];
    for p in positions {
        metas.push(AccountMeta::new(*p, false));
    }
    Instruction::new_with_bytes(PROGRAM_ID, &data, metas)
}

fn ix_update_risk(authority: Address, market: Address, base: u64, prem: u64, thresh: u64) -> Instruction {
    let mut data = vec![ix::UPDATE_RISK_PARAMS];
    data.extend_from_slice(&base.to_le_bytes());
    data.extend_from_slice(&prem.to_le_bytes());
    data.extend_from_slice(&thresh.to_le_bytes());
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(market, false),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn ix_withdraw(
    owner: Address,
    market: Address,
    position: Address,
    vault: Address,
    user_token: Address,
    mint: Address,
    vault_authority: Address,
    amount: u64,
) -> Instruction {
    let mut data = vec![ix::WITHDRAW_COLLATERAL];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(position, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(user_token, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(TOKEN_2022, false),
        ],
    )
}

fn ix_deposit_mm_collateral(
    funder: Address,
    market: Address,
    funder_token: Address,
    vault: Address,
    mint: Address,
    amount: u64,
) -> Instruction {
    let mut data = vec![ix::DEPOSIT_MM_COLLATERAL];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new_readonly(funder, true),
            AccountMeta::new(market, false),
            AccountMeta::new(funder_token, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(TOKEN_2022, false),
        ],
    )
}

fn ix_resolve(authority: Address, market: Address, outcome: u8) -> Instruction {
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &[ix::RESOLVE_MARKET, outcome],
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(market, false),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn ix_claim_winnings(
    owner: Address,
    market: Address,
    position: Address,
    user_token: Address,
    vault: Address,
    mint: Address,
    vault_authority: Address,
) -> Instruction {
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &[ix::CLAIM_WINNINGS],
        vec![
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new_readonly(market, false),
            AccountMeta::new(position, false),
            AccountMeta::new(user_token, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(TOKEN_2022, false),
        ],
    )
}

fn ix_claim_mm_winnings(
    authority: Address,
    market: Address,
    mm_token: Address,
    vault: Address,
    mint: Address,
    vault_authority: Address,
) -> Instruction {
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &[ix::CLAIM_MM_WINNINGS],
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(market, false),
            AccountMeta::new(mm_token, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(TOKEN_2022, false),
        ],
    )
}

fn ix_close_market(authority: Address, market: Address) -> Instruction {
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &[ix::CLOSE_MARKET],
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(market, false),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn ix_withdraw_mm_collateral(
    authority: Address,
    market: Address,
    mm_token: Address,
    vault: Address,
    mint: Address,
    vault_authority: Address,
    amount: u64,
) -> Instruction {
    let mut data = vec![ix::WITHDRAW_MM_COLLATERAL];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(market, false),
            AccountMeta::new(mm_token, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(TOKEN_2022, false),
        ],
    )
}

fn ix_submit_batch_trusted(
    authority: Address,
    market: Address,
    positions: &[Address],
    frame: &[u8],
) -> Instruction {
    let mut data = vec![ix::SUBMIT_BATCH_TRUSTED];
    data.extend_from_slice(frame);
    let mut metas = vec![
        AccountMeta::new_readonly(authority, true),
        AccountMeta::new(market, false),
    ];
    for p in positions {
        metas.push(AccountMeta::new(*p, false));
    }
    Instruction::new_with_bytes(PROGRAM_ID, &data, metas)
}

// ---- composite scenario ----------------------------------------------------------------------

/// A fully initialized market with a 3-of-3... no, 2-of-3 committee, two funded+deposited users.
struct Market {
    authority: Keypair,
    market: Address,
    committee_acc: Address,
    vault: Address,
    mm_account: Address,
    committee_keys: Vec<SigningKey>,
    alice: Keypair,
    alice_pos: Address,
    bob: Keypair,
    bob_pos: Address,
}

const DEPOSIT: u64 = 1_000_000;
/// MM backstop posted in the test market — far more than any single batch's residual obligation.
const MM_BACKSTOP: u64 = 1_000_000;
/// MM backstop reserved by the canonical net-200 residual: `200 * (1 − $0.52)` = `200 * 480_000/1e6`.
const RESERVED: u64 = 96;

fn setup_market(env: &mut Env) -> Market {
    setup_market_inner(env, None)
}

fn setup_market_inner(env: &mut Env, settlement_authority: Option<Address>) -> Market {
    let authority = Keypair::new();
    env.svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    let (market, _) = market_pda(authority.pubkey());
    let (committee_acc, _) = committee_pda(market);
    let (vault_auth, _) = vault_pda(market);

    let vault = env.create_token_account(vault_auth);
    let mm_authority = Keypair::new();
    let mm_account = env.create_token_account(mm_authority.pubkey());

    let keys = committee(3);
    let members = member_bytes(&keys);

    let auth = authority.insecure_clone();
    env.send(
        &[ix_initialize_market(
            env.payer.pubkey(),
            authority.pubkey(),
            env.mint.pubkey(),
            vault,
            mm_account,
            market,
            committee_acc,
            &members,
            3,
            2, // threshold: 2-of-3
            settlement_authority,
        )],
        &[&auth],
    )
    .unwrap();

    // Fund the MM backstop so Tier-2 residuals are fully collateralized for resolution payout.
    env.mint_to(mm_account, MM_BACKSTOP);
    let mm_auth = mm_authority.insecure_clone();
    env.send(
        &[ix_deposit_mm_collateral(
            mm_authority.pubkey(),
            market,
            mm_account,
            vault,
            env.mint.pubkey(),
            MM_BACKSTOP,
        )],
        &[&mm_auth],
    )
    .unwrap();

    // Two users with positions + deposits.
    let (alice, alice_pos) = init_user(env, market);
    let (bob, bob_pos) = init_user(env, market);
    fund_and_deposit(env, market, &alice, alice_pos, vault, DEPOSIT);
    fund_and_deposit(env, market, &bob, bob_pos, vault, DEPOSIT);

    Market {
        authority,
        market,
        committee_acc,
        vault,
        mm_account,
        committee_keys: keys,
        alice,
        alice_pos,
        bob,
        bob_pos,
    }
}

fn init_user(env: &mut Env, market: Address) -> (Keypair, Address) {
    let user = Keypair::new();
    env.svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
    let (pos, _) = position_pda(market, user.pubkey());
    let u = user.insecure_clone();
    env.send(&[ix_init_position(env.payer.pubkey(), user.pubkey(), market, pos)], &[&u])
        .unwrap();
    (user, pos)
}

fn fund_and_deposit(env: &mut Env, market: Address, user: &Keypair, pos: Address, vault: Address, amount: u64) {
    let token = env.create_token_account(user.pubkey());
    env.mint_to(token, amount);
    let u = user.insecure_clone();
    env.send(
        &[ix_deposit(user.pubkey(), market, pos, token, vault, env.mint.pubkey(), amount)],
        &[&u],
    )
    .unwrap();
}

/// Build a submit_batch transaction's instruction list (ed25519 sigs from `signing_keys` + the
/// settlement instruction) and submit it.
#[allow(clippy::too_many_arguments)]
fn submit_batch(
    env: &mut Env,
    m: &Market,
    signing_keys: &[&SigningKey],
    frame: &[u8],
    positions: &[Address],
    relayer: &Keypair,
) -> litesvm::types::TransactionResult {
    let (vault_auth, _) = vault_pda(m.market);
    let mut ixs: Vec<Instruction> = signing_keys.iter().map(|k| ed25519_ix(k, frame)).collect();
    ixs.push(ix_submit_batch(
        relayer.pubkey(),
        m.market,
        m.committee_acc,
        m.vault,
        m.mm_account,
        env.mint.pubkey(),
        vault_auth,
        positions,
        frame,
    ));
    let r = relayer.insecure_clone();
    env.send(&ixs, &[&r])
}

// ---- tests -----------------------------------------------------------------------------------

#[test]
fn init_market_and_positions() {
    let mut env = Env::new();
    let m = setup_market(&mut env);

    let market = env.read_market(m.market);
    assert_eq!(market.authority, *m.authority.pubkey().as_array());
    assert_eq!(market.base_oracle_price, BASE_PRICE);
    assert_eq!(market.imbalance_threshold, THRESHOLD_IMBALANCE);
    assert_eq!(market.last_epoch, 0);

    let alice = env.read_position(m.alice_pos);
    assert_eq!(alice.owner, *m.alice.pubkey().as_array());
    assert_eq!(alice.collateral, DEPOSIT);
    assert_eq!(alice.yes_qty, 0);
}

#[test]
fn deposit_moves_real_tokens() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    // Both users deposited DEPOSIT each + the MM backstop, all into the shared vault.
    assert_eq!(env.token_balance(m.vault), 2 * DEPOSIT + MM_BACKSTOP);
    assert_eq!(env.read_position(m.alice_pos).collateral, DEPOSIT);
    assert_eq!(env.read_market(m.market).mm_collateral, MM_BACKSTOP);
}

/// Full FBA settlement: Alice buys YES (P2P + residual), Bob buys NO (P2P). YES-heavy residual.
fn yes_heavy_frame(m: &Market) -> (Vec<u8>, Vec<Address>) {
    // Alice: 100 P2P-YES + 200 residual-YES. Bob: 100 P2P-NO. net imbalance = 200 (YES-heavy).
    let fills = vec![
        fill(m.alice.pubkey(), 100, 0, 200, 0),
        fill(m.bob.pubkey(), 0, 100, 0, 0),
    ];
    let h = header(m.market, 1, 200, DIRECTION_YES_HEAVY, &fills);
    (frame_bytes(&h, &fills), vec![m.alice_pos, m.bob_pos])
}

#[test]
fn submit_batch_settles_two_tiers_and_reserves_mm_backstop() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let (frame, positions) = yes_heavy_frame(&m);
    submit_batch(
        &mut env,
        &m,
        &[&m.committee_keys[0], &m.committee_keys[1]],
        &frame,
        &positions,
        &relayer,
    )
    .unwrap();

    // Expected economics (6-dec): skew_ratio = 200*1e6/1000 = 200_000; premium = 100_000*0.2 = 20_000;
    // clearing (YES) = 520_000. Alice: t1 = 100*0.5 = 50; t2 = 200*0.52 = 104; cost 154; yes 300.
    // Bob: t1 = 100*0.5 = 50; no 100. MM fee = 200 * 20_000/1e6 = 4; MM takes NO side = 200.
    let alice = env.read_position(m.alice_pos);
    assert_eq!(alice.yes_qty, 300);
    assert_eq!(alice.no_qty, 0);
    assert_eq!(alice.collateral, DEPOSIT - 154);

    let bob = env.read_position(m.bob_pos);
    assert_eq!(bob.no_qty, 100);
    assert_eq!(bob.collateral, DEPOSIT - 50);

    let market = env.read_market(m.market);
    assert_eq!(market.total_yes_supply, 300);
    assert_eq!(market.total_no_supply, 100);
    assert_eq!(market.mm_no, 200);
    assert_eq!(market.mm_yes, 0);
    assert_eq!(market.last_epoch, 1);

    // No spread-fee transfer: the MM's edge is realized in the pricing (it posts less backstop when
    // the premium widens). The vault holds every deposit (users + MM backstop); the MM backstop is
    // reserved by 96 = 200 * (1 − $0.52), fully collateralizing the residual to $1/contract.
    assert_eq!(env.token_balance(m.mm_account), 0);
    assert_eq!(env.token_balance(m.vault), 2 * DEPOSIT + MM_BACKSTOP);
    assert_eq!(market.mm_collateral, MM_BACKSTOP - RESERVED);
}

#[test]
fn submit_batch_no_heavy_residual() {
    // Mirror image: Bob buys NO (P2P + residual), Alice buys YES (P2P). NO-heavy residual.
    // This exercises the DIRECTION_NO_HEAVY match arm (proves the consts are real patterns,
    // not catch-all bindings). clearing(YES)=480_000 → heavy NO price = 520_000.
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let fills = vec![
        fill(m.alice.pubkey(), 100, 0, 0, 0),
        fill(m.bob.pubkey(), 0, 100, 0, 200),
    ];
    let h = header(m.market, 1, 200, protocol::DIRECTION_NO_HEAVY, &fills);
    let frame = frame_bytes(&h, &fills);
    submit_batch(
        &mut env,
        &m,
        &[&m.committee_keys[0], &m.committee_keys[2]],
        &frame,
        &[m.alice_pos, m.bob_pos],
        &relayer,
    )
    .unwrap();

    let alice = env.read_position(m.alice_pos);
    assert_eq!(alice.yes_qty, 100);
    assert_eq!(alice.collateral, DEPOSIT - 50);

    let bob = env.read_position(m.bob_pos);
    assert_eq!(bob.no_qty, 300);
    assert_eq!(bob.collateral, DEPOSIT - 154); // 50 (P2P) + 104 (200 * 0.52)

    let market = env.read_market(m.market);
    assert_eq!(market.total_no_supply, 300);
    assert_eq!(market.total_yes_supply, 100);
    assert_eq!(market.mm_yes, 200);
    assert_eq!(market.mm_no, 0);
    assert_eq!(env.token_balance(m.mm_account), 0);
    assert_eq!(market.mm_collateral, MM_BACKSTOP - RESERVED);
}

#[test]
fn replay_same_epoch_is_rejected() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let (frame, positions) = yes_heavy_frame(&m);
    submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer)
        .unwrap();
    // Same epoch (1) again → StaleEpoch.
    env.svm.expire_blockhash();
    let r = submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer);
    assert!(r.is_err(), "replay of epoch 1 must be rejected");
}

#[test]
fn below_threshold_signatures_is_rejected() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let (frame, positions) = yes_heavy_frame(&m);
    // Only 1 signature, threshold is 2.
    let r = submit_batch(&mut env, &m, &[&m.committee_keys[0]], &frame, &positions, &relayer);
    assert!(r.is_err(), "1-of-2 threshold must be rejected");
}

#[test]
fn non_committee_signer_is_rejected() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let (frame, positions) = yes_heavy_frame(&m);
    // One real committee member + one outsider key (seed 99) → only 1 valid, < threshold 2.
    let mut outsider_seed = [0u8; 32];
    outsider_seed[0] = 99;
    let outsider = SigningKey::from_bytes(&outsider_seed);
    let r = submit_batch(&mut env, &m, &[&m.committee_keys[0], &outsider], &frame, &positions, &relayer);
    assert!(r.is_err(), "an outsider signature must not count toward threshold");
}

#[test]
fn tampered_frame_is_rejected() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let (frame, positions) = yes_heavy_frame(&m);
    // Sign the honest frame, then submit a frame with the net imbalance bumped — signatures no
    // longer cover the submitted bytes, so the committee check fails.
    let sig_a = ed25519_ix(&m.committee_keys[0], &frame);
    let sig_b = ed25519_ix(&m.committee_keys[1], &frame);

    let mut tampered = frame.clone();
    // net_imbalance is at byte offset 48..56 of the header.
    tampered[48..56].copy_from_slice(&201u64.to_le_bytes());

    let (vault_auth, _) = vault_pda(m.market);
    let submit = ix_submit_batch(
        relayer.pubkey(),
        m.market,
        m.committee_acc,
        m.vault,
        m.mm_account,
        env.mint.pubkey(),
        vault_auth,
        &positions,
        &tampered,
    );
    let r = relayer.insecure_clone();
    let res = env.send(&[sig_a, sig_b, submit], &[&r]);
    assert!(res.is_err(), "frame not covered by the signatures must be rejected");
}

#[test]
fn update_risk_params_by_authority() {
    let mut env = Env::new();
    let m = setup_market(&mut env);

    let auth = m.authority.insecure_clone();
    env.send(&[ix_update_risk(m.authority.pubkey(), m.market, 600_000, 50_000, 500)], &[&auth])
        .unwrap();
    let market = env.read_market(m.market);
    assert_eq!(market.base_oracle_price, 600_000);
    assert_eq!(market.max_skew_premium, 50_000);
    assert_eq!(market.imbalance_threshold, 500);

    // A non-authority cannot update.
    let imposter = Keypair::new();
    env.svm.airdrop(&imposter.pubkey(), 1_000_000_000).unwrap();
    let imp = imposter.insecure_clone();
    let r = env.send(&[ix_update_risk(imposter.pubkey(), m.market, 700_000, 1, 1)], &[&imp]);
    assert!(r.is_err(), "non-authority must not update risk params");
}

#[test]
fn withdraw_returns_collateral() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let (vault_auth, _) = vault_pda(m.market);

    let alice_token = env.create_token_account(m.alice.pubkey());
    let a = m.alice.insecure_clone();
    env.send(
        &[ix_withdraw(
            m.alice.pubkey(),
            m.market,
            m.alice_pos,
            m.vault,
            alice_token,
            env.mint.pubkey(),
            vault_auth,
            400_000,
        )],
        &[&a],
    )
    .unwrap();

    assert_eq!(env.token_balance(alice_token), 400_000);
    assert_eq!(env.read_position(m.alice_pos).collateral, DEPOSIT - 400_000);
}

// ---- resolution + winner payout --------------------------------------------------------------

/// Settle the canonical YES-heavy batch, then resolve + claim. Asserts real Token-2022 payouts of
/// $1/contract to winners and that losers (and the losing MM leg) cannot claim.
#[test]
fn resolve_yes_won_pays_yes_holders_only() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();
    let (frame, positions) = yes_heavy_frame(&m); // alice yes 300, bob no 100, mm_no 200
    submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer)
        .unwrap();

    let auth = m.authority.insecure_clone();
    env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).unwrap();
    env.send(&[ix_resolve(m.authority.pubkey(), m.market, OUTCOME_YES_WON)], &[&auth])
        .unwrap();
    assert_eq!(env.read_market(m.market).outcome, OUTCOME_YES_WON);

    let (vault_auth, _) = vault_pda(m.market);
    let vault_before = env.token_balance(m.vault);

    // Alice (300 YES) redeems $1/contract = 300.
    let alice_token = env.create_token_account(m.alice.pubkey());
    let a = m.alice.insecure_clone();
    env.send(
        &[ix_claim_winnings(m.alice.pubkey(), m.market, m.alice_pos, alice_token, m.vault, env.mint.pubkey(), vault_auth)],
        &[&a],
    )
    .unwrap();
    assert_eq!(env.token_balance(alice_token), 300);
    let alice = env.read_position(m.alice_pos);
    assert_eq!(alice.yes_qty, 0);
    assert_eq!(alice.no_qty, 0);
    assert_eq!(env.token_balance(m.vault), vault_before - 300);

    // Bob held NO (the losing side) → nothing to claim.
    let bob_token = env.create_token_account(m.bob.pubkey());
    let b = m.bob.insecure_clone();
    let r = env.send(
        &[ix_claim_winnings(m.bob.pubkey(), m.market, m.bob_pos, bob_token, m.vault, env.mint.pubkey(), vault_auth)],
        &[&b],
    );
    assert!(r.is_err(), "losing side cannot claim");

    // MM backstopped NO (mm_no = 200, the losing side) → nothing to claim.
    let r = env.send(
        &[ix_claim_mm_winnings(m.authority.pubkey(), m.market, m.mm_account, m.vault, env.mint.pubkey(), vault_auth)],
        &[&auth],
    );
    assert!(r.is_err(), "MM losing leg cannot claim");
}

/// Same settled batch, resolved the other way: NO holders + the MM's NO backstop get paid; the YES
/// holder cannot. Exercises the MM payout path and proves vault solvency for both outcomes.
#[test]
fn resolve_no_won_pays_no_holders_and_mm() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();
    let (frame, positions) = yes_heavy_frame(&m); // mm_no = 200
    submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer)
        .unwrap();

    let auth = m.authority.insecure_clone();
    env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).unwrap();
    env.send(&[ix_resolve(m.authority.pubkey(), m.market, OUTCOME_NO_WON)], &[&auth])
        .unwrap();

    let (vault_auth, _) = vault_pda(m.market);

    // Bob (100 NO) redeems 100.
    let bob_token = env.create_token_account(m.bob.pubkey());
    let b = m.bob.insecure_clone();
    env.send(
        &[ix_claim_winnings(m.bob.pubkey(), m.market, m.bob_pos, bob_token, m.vault, env.mint.pubkey(), vault_auth)],
        &[&b],
    )
    .unwrap();
    assert_eq!(env.token_balance(bob_token), 100);

    // MM (mm_no = 200) redeems 200 into its token account.
    env.send(
        &[ix_claim_mm_winnings(m.authority.pubkey(), m.market, m.mm_account, m.vault, env.mint.pubkey(), vault_auth)],
        &[&auth],
    )
    .unwrap();
    assert_eq!(env.token_balance(m.mm_account), 200);
    let market = env.read_market(m.market);
    assert_eq!(market.mm_yes, 0);
    assert_eq!(market.mm_no, 0);

    // Alice held YES (the losing side) → nothing to claim.
    let alice_token = env.create_token_account(m.alice.pubkey());
    let a = m.alice.insecure_clone();
    let r = env.send(
        &[ix_claim_winnings(m.alice.pubkey(), m.market, m.alice_pos, alice_token, m.vault, env.mint.pubkey(), vault_auth)],
        &[&a],
    );
    assert!(r.is_err(), "losing side cannot claim");
}

#[test]
fn resolution_guards() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let (vault_auth, _) = vault_pda(m.market);

    // Non-authority cannot resolve.
    let imposter = Keypair::new();
    env.svm.airdrop(&imposter.pubkey(), 1_000_000_000).unwrap();
    let imp = imposter.insecure_clone();
    let r = env.send(&[ix_resolve(imposter.pubkey(), m.market, OUTCOME_YES_WON)], &[&imp]);
    assert!(r.is_err(), "non-authority cannot resolve");

    // Invalid outcome byte rejected.
    let auth = m.authority.insecure_clone();
    let r = env.send(&[ix_resolve(m.authority.pubkey(), m.market, 7)], &[&auth]);
    assert!(r.is_err(), "invalid outcome rejected");

    // Claiming before resolution is rejected.
    let alice_token = env.create_token_account(m.alice.pubkey());
    let a = m.alice.insecure_clone();
    let r = env.send(
        &[ix_claim_winnings(m.alice.pubkey(), m.market, m.alice_pos, alice_token, m.vault, env.mint.pubkey(), vault_auth)],
        &[&a],
    );
    assert!(r.is_err(), "cannot claim before resolution");

    // Close, resolve once, then a second resolution is rejected (outcomes are final).
    env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).unwrap();
    env.send(&[ix_resolve(m.authority.pubkey(), m.market, OUTCOME_YES_WON)], &[&auth])
        .unwrap();
    env.svm.expire_blockhash();
    let r = env.send(&[ix_resolve(m.authority.pubkey(), m.market, OUTCOME_NO_WON)], &[&auth]);
    assert!(r.is_err(), "cannot re-resolve a settled market");
}

#[test]
fn submit_batch_rejects_when_mm_backstop_insufficient() {
    // A market whose MM never funded the backstop cannot settle a residual batch.
    let mut env = Env::new();
    let authority = Keypair::new();
    env.svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    let (market, _) = market_pda(authority.pubkey());
    let (committee_acc, _) = committee_pda(market);
    let (vault_auth, _) = vault_pda(market);
    let vault = env.create_token_account(vault_auth);
    let mm_authority = Keypair::new();
    let mm_account = env.create_token_account(mm_authority.pubkey());
    let keys = committee(3);
    let members = member_bytes(&keys);
    let auth = authority.insecure_clone();
    env.send(
        &[ix_initialize_market(
            env.payer.pubkey(), authority.pubkey(), env.mint.pubkey(), vault, mm_account, market,
            committee_acc, &members, 3, 2, None,
        )],
        &[&auth],
    )
    .unwrap();
    // (No DepositMmCollateral.) Set up two users and a YES-heavy batch.
    let (alice, alice_pos) = init_user(&mut env, market);
    let (bob, bob_pos) = init_user(&mut env, market);
    fund_and_deposit(&mut env, market, &alice, alice_pos, vault, DEPOSIT);
    fund_and_deposit(&mut env, market, &bob, bob_pos, vault, DEPOSIT);

    let fills = vec![fill(alice.pubkey(), 100, 0, 200, 0), fill(bob.pubkey(), 0, 100, 0, 0)];
    let h = header(market, 1, 200, DIRECTION_YES_HEAVY, &fills);
    let frame = frame_bytes(&h, &fills);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let (vault_auth2, _) = vault_pda(market);
    let mut ixs: Vec<Instruction> = [&keys[0], &keys[1]].iter().map(|k| ed25519_ix(k, &frame)).collect();
    ixs.push(ix_submit_batch(
        relayer.pubkey(), market, committee_acc, vault, mm_account, env.mint.pubkey(), vault_auth2,
        &[alice_pos, bob_pos], &frame,
    ));
    let r = relayer.insecure_clone();
    let res = env.send(&ixs, &[&r]);
    assert!(res.is_err(), "residual settlement must fail without MM backstop collateral");
}

// ---- trusted gateway settlement (the real-Arcium path) ---------------------------------------

/// A market with a registered settlement authority settles a batch via `SubmitBatchTrusted` — no
/// committee signatures, no Ed25519 precompile instructions — and produces the identical result.
#[test]
fn submit_batch_trusted_settles_without_committee() {
    let mut env = Env::new();
    let settlement_auth = Keypair::new();
    env.svm.airdrop(&settlement_auth.pubkey(), 1_000_000_000).unwrap();
    let m = setup_market_inner(&mut env, Some(settlement_auth.pubkey()));

    let (frame, positions) = yes_heavy_frame(&m);
    let sa = settlement_auth.insecure_clone();
    env.send(&[ix_submit_batch_trusted(settlement_auth.pubkey(), m.market, &positions, &frame)], &[&sa])
        .unwrap();

    // Same two-tier settlement as the committee path.
    let alice = env.read_position(m.alice_pos);
    assert_eq!(alice.yes_qty, 300);
    assert_eq!(alice.collateral, DEPOSIT - 154);
    let bob = env.read_position(m.bob_pos);
    assert_eq!(bob.no_qty, 100);
    let market = env.read_market(m.market);
    assert_eq!(market.total_yes_supply, 300);
    assert_eq!(market.mm_no, 200);
    assert_eq!(market.last_epoch, 1);
    assert_eq!(market.mm_collateral, MM_BACKSTOP - RESERVED);
    assert_eq!(market.settlement_authority, *settlement_auth.pubkey().as_array());
}

#[test]
fn submit_batch_trusted_rejects_wrong_authority() {
    let mut env = Env::new();
    let settlement_auth = Keypair::new();
    env.svm.airdrop(&settlement_auth.pubkey(), 1_000_000_000).unwrap();
    let m = setup_market_inner(&mut env, Some(settlement_auth.pubkey()));
    let (frame, positions) = yes_heavy_frame(&m);

    let imposter = Keypair::new();
    env.svm.airdrop(&imposter.pubkey(), 1_000_000_000).unwrap();
    let imp = imposter.insecure_clone();
    let r = env.send(&[ix_submit_batch_trusted(imposter.pubkey(), m.market, &positions, &frame)], &[&imp]);
    assert!(r.is_err(), "only the registered settlement authority may settle on the trusted path");
}

#[test]
fn submit_batch_trusted_disabled_on_committee_only_market() {
    let mut env = Env::new();
    let m = setup_market(&mut env); // no settlement authority configured (committee-only)
    let (frame, positions) = yes_heavy_frame(&m);
    let auth = m.authority.insecure_clone();
    // Even the market admin cannot use the trusted path when it is disabled (zero authority).
    let r = env.send(&[ix_submit_batch_trusted(m.authority.pubkey(), m.market, &positions, &frame)], &[&auth]);
    assert!(r.is_err(), "trusted settlement is disabled when settlement_authority is zero");
}

// ---- market lifecycle, MM withdraw, INVALID refunds, conservation ----------------------------

#[test]
fn close_market_guards_and_gates_lifecycle() {
    let mut env = Env::new();
    let m = setup_market(&mut env);

    // Non-authority cannot close.
    let imposter = Keypair::new();
    env.svm.airdrop(&imposter.pubkey(), 1_000_000_000).unwrap();
    let imp = imposter.insecure_clone();
    assert!(env.send(&[ix_close_market(imposter.pubkey(), m.market)], &[&imp]).is_err());

    // Authority closes; status flips to CLOSED.
    let auth = m.authority.insecure_clone();
    env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).unwrap();
    assert_eq!(env.read_market(m.market).status, STATUS_CLOSED);

    // Double-close is rejected (one-way lifecycle).
    env.svm.expire_blockhash();
    assert!(env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).is_err());
}

#[test]
fn settlement_rejected_after_close() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let auth = m.authority.insecure_clone();
    env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).unwrap();

    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();
    let (frame, positions) = yes_heavy_frame(&m);
    let r = submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer);
    assert!(r.is_err(), "batches must not settle after trading is closed");
}

#[test]
fn resolve_rejected_before_close() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();
    let (frame, positions) = yes_heavy_frame(&m);
    submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer)
        .unwrap();

    // Market is still trading → resolution is rejected until it is closed.
    let auth = m.authority.insecure_clone();
    let r = env.send(&[ix_resolve(m.authority.pubkey(), m.market, OUTCOME_YES_WON)], &[&auth]);
    assert!(r.is_err(), "cannot resolve a market that is still trading");
}

#[test]
fn withdraw_mm_collateral_returns_unreserved_float() {
    let mut env = Env::new();
    let m = setup_market(&mut env); // mm_collateral == MM_BACKSTOP, no batch settled yet
    let (vault_auth, _) = vault_pda(m.market);
    let auth = m.authority.insecure_clone();

    // Withdraw part of the free float to the MM account.
    env.send(
        &[ix_withdraw_mm_collateral(m.authority.pubkey(), m.market, m.mm_account, m.vault, env.mint.pubkey(), vault_auth, 600_000)],
        &[&auth],
    )
    .unwrap();
    assert_eq!(env.token_balance(m.mm_account), 600_000);
    assert_eq!(env.read_market(m.market).mm_collateral, MM_BACKSTOP - 600_000);

    // Cannot withdraw more than the remaining float.
    env.svm.expire_blockhash();
    let r = env.send(
        &[ix_withdraw_mm_collateral(m.authority.pubkey(), m.market, m.mm_account, m.vault, env.mint.pubkey(), vault_auth, MM_BACKSTOP)],
        &[&auth],
    );
    assert!(r.is_err(), "cannot withdraw more MM collateral than the unreserved float");
}

#[test]
fn resolve_invalid_refunds_both_sides_at_midpoint() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();
    let (frame, positions) = yes_heavy_frame(&m); // alice yes 300, bob no 100, mm_no 200
    submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer)
        .unwrap();

    let auth = m.authority.insecure_clone();
    env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).unwrap();
    env.send(&[ix_resolve(m.authority.pubkey(), m.market, OUTCOME_INVALID)], &[&auth]).unwrap();

    let (vault_auth, _) = vault_pda(m.market);

    // Every contract settles at $0.50: Alice 300 YES → 150, Bob 100 NO → 50, MM 200 NO → 100.
    let alice_token = env.create_token_account(m.alice.pubkey());
    let a = m.alice.insecure_clone();
    env.send(&[ix_claim_winnings(m.alice.pubkey(), m.market, m.alice_pos, alice_token, m.vault, env.mint.pubkey(), vault_auth)], &[&a]).unwrap();
    assert_eq!(env.token_balance(alice_token), 150);

    let bob_token = env.create_token_account(m.bob.pubkey());
    let b = m.bob.insecure_clone();
    env.send(&[ix_claim_winnings(m.bob.pubkey(), m.market, m.bob_pos, bob_token, m.vault, env.mint.pubkey(), vault_auth)], &[&b]).unwrap();
    assert_eq!(env.token_balance(bob_token), 50);

    env.send(&[ix_claim_mm_winnings(m.authority.pubkey(), m.market, m.mm_account, m.vault, env.mint.pubkey(), vault_auth)], &[&auth]).unwrap();
    assert_eq!(env.token_balance(m.mm_account), 100);
    // 150 + 50 + 100 = 300 = the locked backing — fully solvent for a voided market.
}

/// End-to-end conservation: through deposit → settle → close → resolve → all claims + all
/// withdrawals, no value is created or destroyed — the vault empties to exactly zero and every token
/// minted into the system is accounted for.
#[test]
fn conservation_invariant_full_lifecycle() {
    let mut env = Env::new();
    let m = setup_market(&mut env);
    let relayer = Keypair::new();
    env.svm.airdrop(&relayer.pubkey(), 1_000_000_000).unwrap();

    let total_in = env.token_balance(m.vault); // 2*DEPOSIT + MM_BACKSTOP
    assert_eq!(total_in, 2 * DEPOSIT + MM_BACKSTOP);

    let (frame, positions) = yes_heavy_frame(&m); // alice yes 300, bob no 100, mm_no 200
    submit_batch(&mut env, &m, &[&m.committee_keys[0], &m.committee_keys[1]], &frame, &positions, &relayer)
        .unwrap();

    let auth = m.authority.insecure_clone();
    env.send(&[ix_close_market(m.authority.pubkey(), m.market)], &[&auth]).unwrap();
    env.send(&[ix_resolve(m.authority.pubkey(), m.market, OUTCOME_YES_WON)], &[&auth]).unwrap();
    let (vault_auth, _) = vault_pda(m.market);

    // Winners claim, then everyone withdraws their remaining free collateral.
    let alice_token = env.create_token_account(m.alice.pubkey());
    let a = m.alice.insecure_clone();
    env.send(&[ix_claim_winnings(m.alice.pubkey(), m.market, m.alice_pos, alice_token, m.vault, env.mint.pubkey(), vault_auth)], &[&a]).unwrap();
    let alice_free = env.read_position(m.alice_pos).collateral;
    env.send(&[ix_withdraw(m.alice.pubkey(), m.market, m.alice_pos, m.vault, alice_token, env.mint.pubkey(), vault_auth, alice_free)], &[&a]).unwrap();

    let bob_token = env.create_token_account(m.bob.pubkey());
    let b = m.bob.insecure_clone();
    let bob_free = env.read_position(m.bob_pos).collateral; // bob's YES lost → only free collateral
    env.send(&[ix_withdraw(m.bob.pubkey(), m.market, m.bob_pos, m.vault, bob_token, env.mint.pubkey(), vault_auth, bob_free)], &[&b]).unwrap();

    let mm_free = env.read_market(m.market).mm_collateral;
    env.send(&[ix_withdraw_mm_collateral(m.authority.pubkey(), m.market, m.mm_account, m.vault, env.mint.pubkey(), vault_auth, mm_free)], &[&auth]).unwrap();

    // The vault is fully drained and every token is recovered by the participants.
    assert_eq!(env.token_balance(m.vault), 0, "vault must empty to exactly zero");
    let recovered = env.token_balance(alice_token) + env.token_balance(bob_token) + env.token_balance(m.mm_account);
    assert_eq!(recovered, total_in, "no value created or destroyed across the full lifecycle");
}
