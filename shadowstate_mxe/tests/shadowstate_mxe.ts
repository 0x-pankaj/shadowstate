import * as anchor from "@anchor-lang/core";
import { Program } from "@anchor-lang/core";
import { PublicKey, Keypair } from "@solana/web3.js";
import { ShadowstateMxe } from "../target/types/shadowstate_mxe";
import { randomBytes } from "crypto";
import {
  awaitComputationFinalization,
  getArciumEnv,
  getCompDefAccOffset,
  getArciumAccountBaseSeed,
  getArciumProgramId,
  getArciumProgram,
  uploadCircuit,
  RescueCipher,
  deserializeLE,
  getMXEPublicKey,
  getMXEAccAddress,
  getMempoolAccAddress,
  getCompDefAccAddress,
  getExecutingPoolAccAddress,
  getComputationAccAddress,
  getClusterAccAddress,
  getLookupTableAddress,
  x25519,
} from "@arcium-hq/client";
import * as fs from "fs";
import * as os from "os";
import { expect } from "chai";

describe("ShadowState confidential matching (devnet)", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.shadowstateMxe as Program<ShadowstateMxe>;
  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const arciumProgram = getArciumProgram(provider);

  const arciumEnv = getArciumEnv();
  const clusterAccount = getClusterAccAddress(arciumEnv.arciumClusterOffset);

  type Event = anchor.IdlEvents<(typeof program)["idl"]>;
  const awaitEvent = async <E extends keyof Event>(eventName: E): Promise<Event[E]> => {
    let listenerId: number;
    const event = await new Promise<Event[E]>((res) => {
      listenerId = program.addEventListener(eventName, (e) => res(e));
    });
    await program.removeEventListener(listenerId);
    return event;
  };

  const owner = readKpJson(`${os.homedir()}/.config/solana/id.json`);
  const market = Keypair.generate().publicKey; // gateway just stores this; settlement is separate
  const epoch = new anchor.BN(1);

  const bookPda = PublicKey.findProgramAddressSync(
    [Buffer.from("book"), market.toBuffer(), epoch.toArrayLike(Buffer, "le", 8)],
    program.programId
  )[0];

  let mxePublicKey: Uint8Array;

  const stdAccs = (compName: string, computationOffset: anchor.BN) => ({
    computationAccount: getComputationAccAddress(arciumEnv.arciumClusterOffset, computationOffset),
    clusterAccount,
    mxeAccount: getMXEAccAddress(program.programId),
    mempoolAccount: getMempoolAccAddress(arciumEnv.arciumClusterOffset),
    executingPool: getExecutingPoolAccAddress(arciumEnv.arciumClusterOffset),
    compDefAccount: getCompDefAccAddress(
      program.programId,
      Buffer.from(getCompDefAccOffset(compName)).readUInt32LE()
    ),
  });

  it("inits comp defs, matches two sealed opposite orders P2P (confidential)", async () => {
    // --- 1. Initialize + upload the three circuits (idempotent) ---
    for (const name of ["init_book", "ingest_order", "clear_batch"]) {
      await initCompDef(name);
    }

    mxePublicKey = await getMXEPublicKeyWithRetry(provider, program.programId);
    console.log("MXE x25519 pubkey:", Buffer.from(mxePublicKey).toString("hex"));

    // --- 2. Open an encrypted batch book ---
    {
      const offset = new anchor.BN(randomBytes(8), "hex");
      await program.methods
        .initBook(offset, market, epoch)
        .accountsPartial({ ...stdAccs("init_book", offset), book: bookPda })
        .rpc({ skipPreflight: true, commitment: "confirmed" });
      await awaitComputationFinalization(provider, offset, program.programId, "confirmed");
      console.log("Book opened (encrypted empty book on-chain).");
    }

    // --- 3. Two clients seal opposite orders. Nobody can read them until clearing. ---
    await ingest(0, 100n); // Alice: BUY YES 100
    await ingest(1, 100n); // Bob:   BUY NO 100
    console.log("Two sealed orders ingested — side & size hidden in the MXE.");

    // --- 4. Close the batch: the MXE matches them and reveals only the cleared result ---
    const clearedPromise = awaitEvent("batchCleared");
    {
      const offset = new anchor.BN(randomBytes(8), "hex");
      await program.methods
        .clearBatch(offset)
        .accountsPartial({ ...stdAccs("clear_batch", offset), book: bookPda })
        .rpc({ skipPreflight: true, commitment: "confirmed" });
      await awaitComputationFinalization(provider, offset, program.programId, "confirmed");
    }
    const cleared = await clearedPromise;

    console.log("BatchCleared:", {
      total_yes: cleared.totalYes.toString(),
      total_no: cleared.totalNo.toString(),
      matched: cleared.matched.toString(),
      net_imbalance: cleared.netImbalance.toString(),
      direction: cleared.direction,
    });

    // The two opposite orders crossed peer-to-peer at the midpoint — no MM, balanced book.
    expect(cleared.totalYes.toNumber()).to.equal(100);
    expect(cleared.totalNo.toNumber()).to.equal(100);
    expect(cleared.matched.toNumber()).to.equal(100);
    expect(cleared.netImbalance.toNumber()).to.equal(0);
  });

  async function ingest(side: number, qty: bigint) {
    const priv = x25519.utils.randomSecretKey();
    const pub = x25519.getPublicKey(priv);
    const cipher = new RescueCipher(x25519.getSharedSecret(priv, mxePublicKey));
    const nonce = randomBytes(16);
    const ct = cipher.encrypt([BigInt(side), qty], nonce);

    const offset = new anchor.BN(randomBytes(8), "hex");
    await program.methods
      .ingestOrder(
        offset,
        Array.from(ct[0]),
        Array.from(ct[1]),
        Array.from(pub),
        new anchor.BN(deserializeLE(nonce).toString())
      )
      .accountsPartial({ ...stdAccs("ingest_order", offset), book: bookPda })
      .rpc({ skipPreflight: true, commitment: "confirmed" });
    await awaitComputationFinalization(provider, offset, program.programId, "confirmed");
  }

  async function initCompDef(name: string) {
    const baseSeed = getArciumAccountBaseSeed("ComputationDefinitionAccount");
    const offset = getCompDefAccOffset(name);
    const compDefPDA = PublicKey.findProgramAddressSync(
      [baseSeed, program.programId.toBuffer(), offset],
      getArciumProgramId()
    )[0];

    const existing = await provider.connection.getAccountInfo(compDefPDA);
    if (existing) {
      console.log(`comp def ${name} already initialized, skipping`);
      return;
    }

    const mxeAccount = getMXEAccAddress(program.programId);
    const mxeAcc = await arciumProgram.account.mxeAccount.fetch(mxeAccount);
    const lutAddress = getLookupTableAddress(program.programId, mxeAcc.lutOffsetSlot);

    const methodName = `init${camel(name)}CompDef`;
    await (program.methods as any)
      [methodName]()
      .accounts({ compDefAccount: compDefPDA, payer: owner.publicKey, mxeAccount, addressLookupTable: lutAddress })
      .signers([owner])
      .rpc({ commitment: "confirmed" });

    const rawCircuit = fs.readFileSync(`build/${name}.arcis`);
    // localnet has no rate limit → full parallel batches. (For the free-RPC devnet path, drop this
    // to ~5; the patched uploader refetches a fresh blockhash per batch so slow uploads don't expire.)
    await uploadCircuit(provider, name, program.programId, rawCircuit, true, 500, {
      skipPreflight: true,
      preflightCommitment: "confirmed",
      commitment: "confirmed",
    });
    console.log(`comp def ${name} initialized + circuit uploaded`);
  }
});

function camel(snake: string): string {
  return snake
    .split("_")
    .map((s) => s.charAt(0).toUpperCase() + s.slice(1))
    .join("");
}

async function getMXEPublicKeyWithRetry(
  provider: anchor.AnchorProvider,
  programId: PublicKey,
  maxRetries = 30,
  retryDelayMs = 1000
): Promise<Uint8Array> {
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      const key = await getMXEPublicKey(provider, programId);
      if (key) return key;
    } catch {}
    if (attempt < maxRetries) await new Promise((r) => setTimeout(r, retryDelayMs));
  }
  throw new Error("MXE public key unavailable");
}

function readKpJson(path: string): Keypair {
  return Keypair.fromSecretKey(new Uint8Array(JSON.parse(fs.readFileSync(path).toString())));
}
