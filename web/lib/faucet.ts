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
 * Mint `amountBase` (6-dec base units) of the test token to the connected wallet. The user pays the
 * fee; the faucet authority co-signs the `mintTo`. Creates the user's ATA if needed.
 */
export async function mintTestTokens(
  connection: Connection,
  wallet: WalletContextState,
  amountBase: bigint
): Promise<string> {
  const auth = faucetAuthority();
  if (!auth) throw new Error("Faucet is not configured for this deployment.");
  if (!wallet.publicKey || !wallet.sendTransaction) throw new Error("Connect a wallet first.");

  const mint = new PublicKey(FAUCET_MINT);
  const owner = wallet.publicKey;
  const ata = userAta(mint, owner);

  const tx = new Transaction()
    .add(
      createAssociatedTokenAccountIdempotentInstruction(
        owner,
        ata,
        owner,
        mint,
        TOKEN_2022_PROGRAM_ID,
        ASSOCIATED_TOKEN_PROGRAM_ID
      )
    )
    .add(createMintToInstruction(mint, ata, auth.publicKey, amountBase, [], TOKEN_2022_PROGRAM_ID));

  tx.feePayer = owner;
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
  tx.recentBlockhash = blockhash;

  // Wallet adds the fee-payer signature; the faucet authority co-signs for the mint.
  const sig = await wallet.sendTransaction(tx, connection, { signers: [auth] });
  await connection.confirmTransaction({ signature: sig, blockhash, lastValidBlockHeight }, "confirmed");
  return sig;
}
