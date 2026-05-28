/**
 * ShadowState confidential order client (real Arcium path).
 *
 * Seals a YES/NO order with the Arcium RescueCipher and submits it to the `shadowstate-arcium-gateway`
 * `ingest_order` instruction — so the order's side and size stay hidden inside the MXE until the batch
 * clears. This is the production counterpart to the Rust `mpc-core::seal` *model*: the real cluster
 * encryption (RescueCipher over x25519) only exists in `@arcium-hq/client`, so this client is
 * TypeScript.
 *
 * ⚠️ Requires the Arcium toolchain to run: `@arcium-hq/client`, `@coral-xyz/anchor`, the deployed
 * gateway program + its generated IDL/types, and a live MXE cluster (devnet offset 456). It is not
 * compiled or tested in this repo's Rust CI. API per the project's `arcium-dev` skill (Arcium 0.6.3).
 */

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair } from "@solana/web3.js";
import { randomBytes, createHash } from "crypto";
import nacl from "tweetnacl";
import {
  RescueCipher,
  x25519,
  deserializeLE,
  getArciumEnv,
  getMXEPublicKey,
  getMXEAccAddress,
  getMempoolAccAddress,
  getExecutingPoolAccAddress,
  getComputationAccAddress,
  getClusterAccAddress,
  getCompDefAccAddress,
  getCompDefAccOffset,
  getFeePoolAccAddress,
  getClockAccAddress,
  awaitComputationFinalization,
} from "@arcium-hq/client";

/** YES = buy YES contracts, NO = buy NO contracts. Matches the circuit's `OrderInput.side`. */
export enum Side {
  Yes = 0,
  No = 1,
}

/** Derive a reproducible x25519 keypair from a Solana wallet (so past data stays decryptable). */
export function deriveEncryptionKey(
  wallet: Keypair,
  label = "shadowstate-order-encryption-v1"
): { privateKey: Uint8Array; publicKey: Uint8Array } {
  const sig = nacl.sign.detached(new TextEncoder().encode(label), wallet.secretKey);
  const privateKey = new Uint8Array(createHash("sha256").update(sig).digest());
  return { privateKey, publicKey: x25519.getPublicKey(privateKey) };
}

/** The MXE public key is published asynchronously after comp-defs init; retry until available. */
export async function getMXEPublicKeyWithRetry(
  provider: anchor.AnchorProvider,
  programId: PublicKey,
  maxRetries = 20,
  retryDelayMs = 500
): Promise<Uint8Array> {
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      const key = await getMXEPublicKey(provider, programId);
      if (key) return key;
    } catch {
      /* not ready yet */
    }
    if (attempt < maxRetries) await new Promise((r) => setTimeout(r, retryDelayMs));
  }
  throw new Error(`MXE public key unavailable after ${maxRetries} attempts`);
}

/** The book PDA: `[b"book", market, epoch_le]` (must match the gateway's seeds). */
export function bookPda(programId: PublicKey, market: PublicKey, epoch: bigint): PublicKey {
  const epochLe = Buffer.alloc(8);
  epochLe.writeBigUInt64LE(epoch);
  return PublicKey.findProgramAddressSync([Buffer.from("book"), market.toBuffer(), epochLe], programId)[0];
}

const MAX_RETRIES = 5;
const RETRY_DELAY_MS = 3000;

/**
 * Seal an order and submit it to the gateway's `ingest_order`, retrying with a **fresh computation
 * offset per attempt** (offsets are one-time-use; MPC computations can abort on node disagreement).
 * Resolves to the finalization signature once the MXE has folded the order into the encrypted book.
 */
export async function submitSealedOrder(
  program: Program,
  provider: anchor.AnchorProvider,
  owner: Keypair,
  market: PublicKey,
  epoch: bigint,
  side: Side,
  qty: bigint
): Promise<string> {
  const mxePublicKey = await getMXEPublicKeyWithRetry(provider, program.programId);
  const keys = deriveEncryptionKey(owner);
  const cipher = new RescueCipher(x25519.getSharedSecret(keys.privateKey, mxePublicKey));

  // Encrypt OrderInput { side: u8, qty: u64 } → two field-element ciphertexts under one nonce.
  const nonce = randomBytes(16);
  const ciphertext = cipher.encrypt([BigInt(side), qty], nonce);
  const sideCt = Array.from(ciphertext[0]) as number[];
  const qtyCt = Array.from(ciphertext[1]) as number[];
  const nonceBN = new anchor.BN(deserializeLE(nonce).toString());

  const arciumEnv = getArciumEnv();
  const clusterOffset = arciumEnv.arciumClusterOffset;
  const book = bookPda(program.programId, market, epoch);

  for (let attempt = 1; attempt <= MAX_RETRIES; attempt++) {
    const computationOffset = new anchor.BN(randomBytes(8), "hex");
    try {
      await program.methods
        .ingestOrder(computationOffset, sideCt, qtyCt, Array.from(keys.publicKey) as number[], nonceBN)
        .accountsPartial({
          payer: owner.publicKey,
          mxeAccount: getMXEAccAddress(program.programId),
          mempoolAccount: getMempoolAccAddress(clusterOffset),
          executingPool: getExecutingPoolAccAddress(clusterOffset),
          computationAccount: getComputationAccAddress(clusterOffset, computationOffset),
          compDefAccount: getCompDefAccAddress(
            program.programId,
            Buffer.from(getCompDefAccOffset("ingest_order")).readUInt32LE()
          ),
          clusterAccount: getClusterAccAddress(clusterOffset),
          poolAccount: getFeePoolAccAddress(),
          clockAccount: getClockAccAddress(),
          book,
        })
        .signers([owner])
        .rpc({ skipPreflight: true, commitment: "confirmed" });

      const finalizeSig = await awaitComputationFinalization(
        provider,
        computationOffset,
        program.programId,
        "confirmed"
      );
      const txResult = await provider.connection.getTransaction(finalizeSig, {
        commitment: "confirmed",
        maxSupportedTransactionVersion: 0,
      });
      if (txResult?.meta?.err) {
        if (attempt < MAX_RETRIES) {
          await new Promise((r) => setTimeout(r, RETRY_DELAY_MS));
          continue;
        }
        throw new Error(`ingest_order aborted after ${MAX_RETRIES} attempts`);
      }
      return finalizeSig;
    } catch (err) {
      if (attempt >= MAX_RETRIES) throw err;
      await new Promise((r) => setTimeout(r, RETRY_DELAY_MS));
    }
  }
  throw new Error("ingest_order: exhausted all retries");
}
