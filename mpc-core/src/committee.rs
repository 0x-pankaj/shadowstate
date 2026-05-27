//! The MPC node matrix's signing identities and threshold frame-signing.
//!
//! Each node holds an Ed25519 signing key; the public halves are registered once on-chain in the
//! immutable `Committee` account at `InitializeMarket`. To finalize a batch, at least `threshold`
//! distinct nodes sign the exact frame bytes. The relay turns each signature into an Ed25519
//! precompile instruction; the on-chain `sig::verify_committee` authorizes which keys signed which
//! message and requires `≥ threshold` distinct committee signers.

use {
    crate::error::{MpcError, Result},
    ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey},
    protocol::MAX_COMMITTEE,
};

/// One node's signature over a frame: the raw 32-byte public key and 64-byte signature, in exactly
/// the form the Ed25519 precompile (and the on-chain parser) consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSignature {
    /// Signer's Ed25519 public key (must be a registered committee member on-chain).
    pub pubkey: [u8; 32],
    /// Detached Ed25519 signature over the frame bytes.
    pub signature: [u8; 64],
}

/// The decentralized signing committee. Holds the node signing keys and the on-chain threshold.
pub struct Committee {
    keys: Vec<SigningKey>,
    threshold: u8,
}

impl Committee {
    /// Build a committee from per-node 32-byte seeds and a signing threshold.
    ///
    /// Enforces the same bounds the on-chain `InitializeMarket` enforces: `1 ≤ count ≤ MAX_COMMITTEE`
    /// and `1 ≤ threshold ≤ count`.
    pub fn from_seeds(seeds: &[[u8; 32]], threshold: u8) -> Result<Self> {
        let count = seeds.len();
        if count == 0 || count > MAX_COMMITTEE {
            return Err(MpcError::InvalidCommittee);
        }
        if threshold == 0 || threshold as usize > count {
            return Err(MpcError::InvalidCommittee);
        }
        let keys = seeds.iter().map(SigningKey::from_bytes).collect();
        Ok(Self { keys, threshold })
    }

    /// Number of nodes.
    #[inline]
    pub fn count(&self) -> usize {
        self.keys.len()
    }

    /// Required signing threshold.
    #[inline]
    pub fn threshold(&self) -> u8 {
        self.threshold
    }

    /// The `i`-th member's public key bytes.
    pub fn member_pubkey(&self, i: usize) -> Option<[u8; 32]> {
        self.keys.get(i).map(|k| k.verifying_key().to_bytes())
    }

    /// All member public keys, in registration order.
    pub fn member_pubkeys(&self) -> Vec<[u8; 32]> {
        self.keys.iter().map(|k| k.verifying_key().to_bytes()).collect()
    }

    /// The concatenated `count * 32` member-key bytes, in the exact layout `InitializeMarket`
    /// expects for the committee member array.
    pub fn member_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.keys.len() * 32);
        for k in &self.keys {
            out.extend_from_slice(&k.verifying_key().to_bytes());
        }
        out
    }

    /// Sign `frame` with the members at `indices`. Each index must be in range.
    pub fn sign_with(&self, frame: &[u8], indices: &[usize]) -> Result<Vec<NodeSignature>> {
        let mut sigs = Vec::with_capacity(indices.len());
        for &i in indices {
            let key = self.keys.get(i).ok_or(MpcError::InvalidCommittee)?;
            sigs.push(NodeSignature {
                pubkey: key.verifying_key().to_bytes(),
                signature: key.sign(frame).to_bytes(),
            });
        }
        Ok(sigs)
    }

    /// Sign `frame` with the first `threshold` members — the minimal quorum that finalizes a batch.
    pub fn sign_threshold(&self, frame: &[u8]) -> Result<Vec<NodeSignature>> {
        let indices: Vec<usize> = (0..self.threshold as usize).collect();
        self.sign_with(frame, &indices)
    }

    /// Sign `frame` with every member (full unanimity).
    pub fn sign_all(&self, frame: &[u8]) -> Result<Vec<NodeSignature>> {
        let indices: Vec<usize> = (0..self.keys.len()).collect();
        self.sign_with(frame, &indices)
    }
}

/// Off-chain mirror of the on-chain authorization check: count the **distinct, registered** members
/// whose signature over `message` is valid, and confirm it meets `threshold`. Lets the relayer
/// pre-flight a batch before paying to submit it. `members` is the on-chain committee public-key set.
pub fn quorum_is_met(
    members: &[[u8; 32]],
    threshold: u8,
    message: &[u8],
    sigs: &[NodeSignature],
) -> bool {
    let mut seen: u64 = 0; // bitmask over member indices (MAX_COMMITTEE ≤ 64)
    for s in sigs {
        let Some(idx) = members.iter().position(|m| m == &s.pubkey) else {
            continue;
        };
        let Ok(vk) = VerifyingKey::from_bytes(&s.pubkey) else {
            continue;
        };
        let Ok(sig) = Signature::from_slice(&s.signature) else {
            continue;
        };
        if vk.verify(message, &sig).is_ok() {
            seen |= 1u64 << idx;
        }
    }
    seen.count_ones() >= threshold as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeds(n: u8) -> Vec<[u8; 32]> {
        (1..=n)
            .map(|i| {
                let mut s = [0u8; 32];
                s[0] = i;
                s
            })
            .collect()
    }

    #[test]
    fn rejects_bad_sizes() {
        assert!(Committee::from_seeds(&[], 1).is_err());
        assert!(Committee::from_seeds(&seeds(3), 0).is_err());
        assert!(Committee::from_seeds(&seeds(3), 4).is_err());
        assert!(Committee::from_seeds(&seeds((MAX_COMMITTEE + 1) as u8), 1).is_err());
    }

    #[test]
    fn member_bytes_layout_matches_pubkeys() {
        let c = Committee::from_seeds(&seeds(3), 2).unwrap();
        let bytes = c.member_bytes();
        assert_eq!(bytes.len(), 3 * 32);
        for (i, pk) in c.member_pubkeys().iter().enumerate() {
            assert_eq!(&bytes[i * 32..i * 32 + 32], pk);
        }
    }

    #[test]
    fn threshold_signatures_meet_quorum() {
        let c = Committee::from_seeds(&seeds(3), 2).unwrap();
        let frame = b"batch-frame-bytes";
        let sigs = c.sign_threshold(frame).unwrap();
        assert_eq!(sigs.len(), 2);
        assert!(quorum_is_met(&c.member_pubkeys(), c.threshold(), frame, &sigs));
    }

    #[test]
    fn below_threshold_fails_quorum() {
        let c = Committee::from_seeds(&seeds(3), 2).unwrap();
        let frame = b"f";
        let one = c.sign_with(frame, &[0]).unwrap();
        assert!(!quorum_is_met(&c.member_pubkeys(), c.threshold(), frame, &one));
    }

    #[test]
    fn outsider_signature_does_not_count() {
        let c = Committee::from_seeds(&seeds(3), 2).unwrap();
        let outsider = Committee::from_seeds(&seeds(1), 1).unwrap(); // seed 1 collides; use a fresh one
        let mut s = [0u8; 32];
        s[0] = 99;
        let stranger = Committee::from_seeds(&[s], 1).unwrap();
        let frame = b"frame";
        let mut sigs = c.sign_with(frame, &[0]).unwrap();
        sigs.extend(stranger.sign_with(frame, &[0]).unwrap());
        // One real + one outsider = 1 valid < threshold 2.
        assert!(!quorum_is_met(&c.member_pubkeys(), c.threshold(), frame, &sigs));
        let _ = outsider;
    }

    #[test]
    fn signature_over_wrong_message_does_not_count() {
        let c = Committee::from_seeds(&seeds(3), 2).unwrap();
        let sigs = c.sign_threshold(b"honest-frame").unwrap();
        assert!(!quorum_is_met(&c.member_pubkeys(), c.threshold(), b"tampered-frame", &sigs));
    }
}
