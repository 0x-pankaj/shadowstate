import { Connection, PublicKey, Transaction } from "@solana/web3.js";
import { GATEWAY_PROGRAM_ID } from "./constants";
import { bookPda } from "./pdas";

export interface WalletLike {
  publicKey: PublicKey;
  signTransaction<T extends Transaction>(tx: T): Promise<T>;
  signAllTransactions<T extends Transaction>(txs: T[]): Promise<T[]>;
}

/**
 * Seal a YES/NO order with the Arcium RescueCipher and submit it to the gateway's `ingest_order`.
 * The order's side + size stay encrypted in the MXE until the batch clears — nothing is exposed
 * on-chain. Heavy Arcium/Anchor deps are loaded lazily so they stay out of the initial bundle.
 *
 * Requires: the gateway's `ingest_order` comp-def initialized, the MXE keys generated, and a `book`
 * already opened for `(market, epoch)`. Resolves to the queue-transaction signature.
 */
export async function placeSealedOrder(opts: {
  connection: Connection;
  wallet: WalletLike;
  market: PublicKey;
  epoch: bigint;
  side: 0 | 1;
  qty: bigint;
}): Promise<string> {
  const { connection, wallet, market, epoch, side, qty } = opts;

  const anchor = await import("@anchor-lang/core");
  const arc = await import("@arcium-hq/client");
  const idl = (await import("./idl/shadowstate_mxe.json")).default as unknown;

  const provider = new anchor.AnchorProvider(connection as any, wallet as any, { commitment: "confirmed" });
  const program = new anchor.Program(idl as any, provider as any) as any;

  // 1. Fetch the MXE x25519 public key and derive a per-order shared secret.
  const mxePubkey = await arc.getMXEPublicKey(provider as any, GATEWAY_PROGRAM_ID);
  if (!mxePubkey) throw new Error("MXE encryption key not available yet — is the MXE keygen finalized?");
  const priv = arc.x25519.utils.randomSecretKey();
  const pub = arc.x25519.getPublicKey(priv);
  const cipher = new arc.RescueCipher(arc.x25519.getSharedSecret(priv, mxePubkey));

  // 2. Seal { side, qty } → two ciphertexts under one nonce.
  const nonce = crypto.getRandomValues(new Uint8Array(16));
  const ct = cipher.encrypt([BigInt(side), qty], nonce);
  const offsetBytes = crypto.getRandomValues(new Uint8Array(8));
  const computationOffset = new anchor.BN(offsetBytes);

  const env = arc.getArciumEnv();
  const cluster = env.arciumClusterOffset;

  // 3. Build + send the queue transaction (sealed order → encrypted book).
  const sig: string = await program.methods
    .ingestOrder(
      computationOffset,
      Array.from(ct[0]),
      Array.from(ct[1]),
      Array.from(pub),
      new anchor.BN(arc.deserializeLE(nonce).toString())
    )
    .accountsPartial({
      computationAccount: arc.getComputationAccAddress(cluster, computationOffset),
      clusterAccount: arc.getClusterAccAddress(cluster),
      mxeAccount: arc.getMXEAccAddress(GATEWAY_PROGRAM_ID),
      mempoolAccount: arc.getMempoolAccAddress(cluster),
      executingPool: arc.getExecutingPoolAccAddress(cluster),
      compDefAccount: arc.getCompDefAccAddress(
        GATEWAY_PROGRAM_ID,
        Buffer.from(arc.getCompDefAccOffset("ingest_order")).readUInt32LE()
      ),
      book: bookPda(market, epoch),
    })
    .rpc({ skipPreflight: true, commitment: "confirmed" });

  return sig;
}
