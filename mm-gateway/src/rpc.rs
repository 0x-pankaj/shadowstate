//! Minimal Solana JSON-RPC submitter — enough to fetch a blockhash and broadcast the signed
//! `UpdateRiskParams` transaction, without pulling the full `solana-client` stack.
//!
//! [`ChainSubmitter`] abstracts the broadcast so the gateway's retune logic is testable with
//! [`MockSubmitter`]; [`RpcSubmitter`] is the real `reqwest` adapter. The request/response shaping is
//! factored into pure helpers that are unit-tested directly.

use {
    crate::error::{GatewayError, Result},
    base64::{engine::general_purpose::STANDARD, Engine},
    serde_json::{json, Value},
    solana_hash::Hash,
    solana_transaction::Transaction,
    std::{future::Future, str::FromStr, sync::Mutex},
};

/// Broadcasts signed transactions and supplies recent blockhashes.
pub trait ChainSubmitter {
    /// Fetch a recent blockhash to stamp into a transaction.
    fn latest_blockhash(&self) -> impl Future<Output = Result<Hash>>;
    /// Broadcast a signed transaction; returns the transaction signature string.
    fn submit_transaction(&self, tx: &Transaction) -> impl Future<Output = Result<String>>;
}

// ---- pure JSON-RPC shaping (unit-tested without network) --------------------------------------

/// Build a JSON-RPC 2.0 request body.
fn rpc_request(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
}

/// `sendTransaction` params: `[base64(tx), {"encoding":"base64"}]`.
fn send_transaction_params(tx: &Transaction) -> Result<Value> {
    let wire = bincode::serialize(tx).map_err(|e| GatewayError::Rpc(e.to_string()))?;
    let b64 = STANDARD.encode(wire);
    Ok(json!([b64, { "encoding": "base64" }]))
}

/// Extract `result.value.blockhash` from a `getLatestBlockhash` response and parse it.
fn parse_blockhash(resp: &Value) -> Result<Hash> {
    let s = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("blockhash"))
        .and_then(|b| b.as_str())
        .ok_or_else(|| GatewayError::Rpc("missing result.value.blockhash".into()))?;
    Hash::from_str(s).map_err(|e| GatewayError::Rpc(format!("bad blockhash: {e}")))
}

/// Extract the signature string from a `sendTransaction` response (or surface an RPC error).
fn parse_signature(resp: &Value) -> Result<String> {
    if let Some(err) = resp.get("error") {
        return Err(GatewayError::Rpc(err.to_string()));
    }
    resp.get("result")
        .and_then(|r| r.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| GatewayError::Rpc("missing result signature".into()))
}

/// Real JSON-RPC submitter over `reqwest`.
pub struct RpcSubmitter {
    url: String,
    client: reqwest::Client,
}

impl RpcSubmitter {
    /// Build a submitter targeting an RPC endpoint (e.g. `https://api.mainnet-beta.solana.com`).
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            client: reqwest::Client::new(),
        }
    }

    async fn call(&self, body: Value) -> Result<Value> {
        self.client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| GatewayError::Rpc(e.to_string()))?
            .json::<Value>()
            .await
            .map_err(|e| GatewayError::Rpc(e.to_string()))
    }
}

impl ChainSubmitter for RpcSubmitter {
    async fn latest_blockhash(&self) -> Result<Hash> {
        let resp = self
            .call(rpc_request("getLatestBlockhash", json!([{ "commitment": "finalized" }])))
            .await?;
        parse_blockhash(&resp)
    }

    async fn submit_transaction(&self, tx: &Transaction) -> Result<String> {
        let resp = self
            .call(rpc_request("sendTransaction", send_transaction_params(tx)?))
            .await?;
        parse_signature(&resp)
    }
}

/// In-memory submitter for tests: hands out a fixed blockhash and records broadcast transactions.
pub struct MockSubmitter {
    blockhash: Hash,
    submitted: Mutex<Vec<Transaction>>,
}

impl MockSubmitter {
    /// A submitter returning `blockhash` from [`ChainSubmitter::latest_blockhash`].
    pub fn new(blockhash: Hash) -> Self {
        Self {
            blockhash,
            submitted: Mutex::new(Vec::new()),
        }
    }

    /// Snapshot of broadcast transactions.
    pub fn submitted(&self) -> Vec<Transaction> {
        self.submitted.lock().expect("submitter mutex").clone()
    }
}

impl ChainSubmitter for MockSubmitter {
    async fn latest_blockhash(&self) -> Result<Hash> {
        Ok(self.blockhash.clone())
    }

    async fn submit_transaction(&self, tx: &Transaction) -> Result<String> {
        self.submitted.lock().expect("submitter mutex").push(tx.clone());
        // Return the transaction's own signature string, as a real RPC would.
        Ok(tx.signatures.first().map(|s| s.to_string()).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_is_well_formed() {
        let req = rpc_request("getLatestBlockhash", json!([{ "commitment": "finalized" }]));
        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["method"], "getLatestBlockhash");
        assert_eq!(req["params"][0]["commitment"], "finalized");
    }

    #[test]
    fn parses_blockhash_from_rpc_shape() {
        // 32 zero bytes base58-encode to "11111111111111111111111111111111".
        let resp = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "context": { "slot": 1 }, "value": { "blockhash": "11111111111111111111111111111111", "lastValidBlockHeight": 100 } }
        });
        let hash = parse_blockhash(&resp).unwrap();
        assert_eq!(hash, Hash::new_from_array([0u8; 32]));
    }

    #[test]
    fn missing_blockhash_is_an_error() {
        let resp = json!({ "result": { "value": {} } });
        assert!(matches!(parse_blockhash(&resp), Err(GatewayError::Rpc(_))));
    }

    #[test]
    fn parses_signature_and_surfaces_errors() {
        let ok = json!({ "result": "5xY...sig" });
        assert_eq!(parse_signature(&ok).unwrap(), "5xY...sig");
        let err = json!({ "error": { "code": -32002, "message": "blockhash not found" } });
        assert!(matches!(parse_signature(&err), Err(GatewayError::Rpc(_))));
    }

    #[test]
    fn send_transaction_params_encode_base64() {
        let tx = Transaction::default();
        let params = send_transaction_params(&tx).unwrap();
        assert_eq!(params[1]["encoding"], "base64");
        // The encoded payload round-trips as base64.
        let b64 = params[0].as_str().unwrap();
        assert!(STANDARD.decode(b64).is_ok());
    }
}
