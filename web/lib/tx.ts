import { Connection, PublicKey, Transaction, TransactionInstruction } from "@solana/web3.js";
import { WalletContextState } from "@solana/wallet-adapter-react";

/** Assemble, sign (via wallet adapter) and confirm a transaction from a list of instructions. */
export async function sendIxs(
  connection: Connection,
  wallet: WalletContextState,
  ixs: TransactionInstruction[],
  feePayer: PublicKey
): Promise<string> {
  if (!wallet.sendTransaction) throw new Error("Wallet does not support sendTransaction.");
  const tx = new Transaction();
  for (const ix of ixs) tx.add(ix);
  tx.feePayer = feePayer;
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
  tx.recentBlockhash = blockhash;
  const sig = await wallet.sendTransaction(tx, connection);
  await connection.confirmTransaction({ signature: sig, blockhash, lastValidBlockHeight }, "confirmed");
  return sig;
}

/** Cluster-aware explorer link for a signature. */
export function explorerTx(sig: string): string {
  const rpc = process.env.NEXT_PUBLIC_RPC_URL || "";
  const cluster = rpc.includes("mainnet") ? "" : "?cluster=devnet";
  return `https://explorer.solana.com/tx/${sig}${cluster}`;
}
