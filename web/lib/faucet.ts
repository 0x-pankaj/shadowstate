import { Connection, Keypair, PublicKey, Transaction } from "@solana/web3.js";
import { WalletContextState } from "@solana/wallet-adapter-react";
import {
  TOKEN_2022_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createMintToInstruction,
} from "@solana/spl-token";
import { userAta } from "./token";

/** ShadowState's test "USDC" mint (devnet). Set by `scripts/setup-faucet.mjs`. */
export const FAUCET_MINT = process.env.NEXT_PUBLIC_FAUCET_MINT || "";

/**
 * Throwaway devnet mint-authority secret (JSON byte array). Safe to expose: its only power is
 * minting a worthless test token. Absent ⇒ the faucet is disabled.
 */
const FAUCET_AUTHORITY_SECRET = process.env.NEXT_PUBLIC_FAUCET_AUTHORITY_SECRET || "";

function faucetAuthority(): Keypair | null {
  if (!FAUCET_AUTHORITY_SECRET) return null;
  try {
    return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(FAUCET_AUTHORITY_SECRET)));
  } catch {
    return null;
  }
}

/** Is the in-app faucet wired (mint + authority present)? */
export function faucetEnabled(): boolean {
  return !!FAUCET_MINT && !!faucetAuthority();
}

/** Does `mint` match the faucet token (so minting it is useful as this market's collateral)? */
export function isFaucetMint(mint: PublicKey | null): boolean {
  return !!mint && !!FAUCET_MINT && mint.toBase58() === FAUCET_MINT;
}

/**
 * Mint `amountBase` (6-dec base units) of the test token to the connected wallet.
 *
 * Self-signing faucet: the throwaway faucet authority is BOTH the fee payer and the mint
 * authority, so the transaction has a single signer and never touches the user's wallet. This
 * avoids Phantom's "Unexpected error" — Phantom refuses to sign a legacy transaction that has a
 * second required signer (the mint authority), failing instantly with no popup. Because the
 * faucet key is a worthless devnet test-token authority, signing in-browser is acceptable here.
 *
 * Requires the faucet authority to hold a little devnet SOL for fees + the user's ATA rent
 * (~0.002 SOL, one-time per user). Fund it with `node scripts/fund-faucet.mjs`.
 */
export async function mintTestTokens(
  connection: Connection,
  wallet: WalletContextState,
  amountBase: bigint
): Promise<string> {
  const auth = faucetAuthority();
  if (!auth) throw new Error("Faucet is not configured for this deployment.");
  if (!wallet.publicKey) throw new Error("Connect a wallet first.");

  const mint = new PublicKey(FAUCET_MINT);
  const owner = wallet.publicKey; // recipient; receives the minted tokens, signs nothing
  const ata = userAta(mint, owner);

  const tx = new Transaction()
    .add(
      // Faucet authority pays rent to create the user's ATA if it doesn't exist yet.
      createAssociatedTokenAccountIdempotentInstruction(
        auth.publicKey,
        ata,
        owner,
        mint,
        TOKEN_2022_PROGRAM_ID,
        ASSOCIATED_TOKEN_PROGRAM_ID
      )
    )
    .add(createMintToInstruction(mint, ata, auth.publicKey, amountBase, [], TOKEN_2022_PROGRAM_ID));

  tx.feePayer = auth.publicKey;
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
  tx.recentBlockhash = blockhash;
  tx.sign(auth);

  // Simulate first so a concrete reason + program logs surface instead of an opaque send error
  // (e.g. faucet authority out of SOL — run `node scripts/fund-faucet.mjs`).
  const sim = await connection.simulateTransaction(tx);
  if (sim.value.err) {
    const logs = (sim.value.logs ?? []).join("\n");
    throw new Error(`Mint simulation failed (${JSON.stringify(sim.value.err)}).\n${logs}`);
  }

  const sig = await connection.sendRawTransaction(tx.serialize());
  await connection.confirmTransaction({ signature: sig, blockhash, lastValidBlockHeight }, "confirmed");
  return sig;
}
