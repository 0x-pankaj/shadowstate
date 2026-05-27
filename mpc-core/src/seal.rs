//! Client-side order sealing — the confidentiality boundary before the share domain.
//!
//! A client performs an x25519 ECDH against the MXE cluster's public key and encrypts the canonical
//! order bytes with ChaCha20Poly1305. The sealed blob is what lands in an on-chain ingestion
//! account; only the cluster (holding the matching static secret) can open it, and it does so only
//! inside the secure context, immediately splitting the recovered quantities into secret shares.
//! This mirrors how an Arcium client encrypts inputs to an MXE.
//!
//! Sealed blob layout (fixed, little-endian):
//! ```text
//!   [ ephemeral_public : 32 ] [ nonce : 12 ] [ ciphertext+tag : plaintext_len + 16 ]
//! ```
//!
//! Key derivation note: we use the raw 32-byte x25519 shared secret as the ChaCha key. A production
//! deployment should run it through an HKDF with a domain-separation label; that is a drop-in change
//! (add `hkdf`) and does not affect this crate's interfaces.

use {
    crate::{
        error::{MpcError, Result},
        order::{Order, ORDER_WIRE_LEN},
    },
    chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Key, Nonce,
    },
    rand::{CryptoRng, RngCore},
    x25519_dalek::{PublicKey, StaticSecret},
};

const EPH_PK_OFF: usize = 0;
const NONCE_OFF: usize = 32;
const CT_OFF: usize = 44;
/// AEAD tag length appended by ChaCha20Poly1305.
const TAG_LEN: usize = 16;

/// Total sealed-blob length for an order: 32 + 12 + (81 + 16).
pub const SEALED_ORDER_LEN: usize = CT_OFF + ORDER_WIRE_LEN + TAG_LEN;

/// The MXE cluster's long-lived decryption key. The *public* half is published; clients seal to it.
#[derive(Clone)]
pub struct ClusterKey {
    secret: StaticSecret,
}

impl ClusterKey {
    /// Deterministic cluster key from a 32-byte seed (reproducible deployments / tests).
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            secret: StaticSecret::from(seed),
        }
    }

    /// Fresh cluster key from a CSPRNG.
    pub fn generate<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        Self {
            secret: StaticSecret::random_from_rng(rng),
        }
    }

    /// The published public key clients seal their orders to.
    pub fn public_bytes(&self) -> [u8; 32] {
        PublicKey::from(&self.secret).to_bytes()
    }

    /// Open a sealed order. Fails on a truncated blob (`MalformedSealedOrder`), a failed
    /// authentication tag (`SealOpenFailed`), or invalid inner order bytes (`MalformedOrder`).
    pub fn open(&self, blob: &[u8]) -> Result<Order> {
        if blob.len() < CT_OFF + TAG_LEN {
            return Err(MpcError::MalformedSealedOrder);
        }
        let mut eph = [0u8; 32];
        eph.copy_from_slice(&blob[EPH_PK_OFF..EPH_PK_OFF + 32]);
        let eph_public = PublicKey::from(eph);

        let shared = self.secret.diffie_hellman(&eph_public);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(shared.as_bytes()));
        let nonce = Nonce::from_slice(&blob[NONCE_OFF..NONCE_OFF + 12]);

        let plaintext = cipher
            .decrypt(nonce, &blob[CT_OFF..])
            .map_err(|_| MpcError::SealOpenFailed)?;
        Order::from_bytes(&plaintext)
    }
}

/// Seal `order` to `cluster_public` using a caller-supplied ephemeral secret + nonce. This is the
/// deterministic core (used by tests); production callers use [`seal_order`].
pub fn seal_order_with(
    order: &Order,
    cluster_public: &[u8; 32],
    ephemeral_secret: StaticSecret,
    nonce_bytes: [u8; 12],
) -> Vec<u8> {
    let cluster_public = PublicKey::from(*cluster_public);
    let shared = ephemeral_secret.diffie_hellman(&cluster_public);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(shared.as_bytes()));
    let nonce = Nonce::from_slice(&nonce_bytes);

    // ChaCha20Poly1305 over a fixed-length plaintext never fails; `expect` here is unreachable and
    // only guards against an allocation fault, which would already have aborted.
    let ciphertext = cipher
        .encrypt(nonce, order.to_bytes().as_ref())
        .expect("chacha20poly1305 encryption of fixed-length order");

    let eph_public = PublicKey::from(&ephemeral_secret).to_bytes();
    let mut blob = Vec::with_capacity(CT_OFF + ciphertext.len());
    blob.extend_from_slice(&eph_public);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    blob
}

/// Seal `order` to `cluster_public`, drawing a fresh ephemeral key and nonce from `rng` (production
/// client path).
pub fn seal_order<R: RngCore + CryptoRng>(
    order: &Order,
    cluster_public: &[u8; 32],
    rng: &mut R,
) -> Vec<u8> {
    let ephemeral_secret = StaticSecret::random_from_rng(&mut *rng);
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    seal_order_with(order, cluster_public, ephemeral_secret, nonce_bytes)
}

#[cfg(test)]
mod tests {
    use {super::*, crate::order::Side, x25519_dalek::StaticSecret};

    fn sample_order() -> Order {
        Order {
            user: [3u8; 32],
            market: [8u8; 32],
            side: Side::Yes,
            qty: 750_000,
            limit_price: 600_000,
        }
    }

    #[test]
    fn seal_then_open_roundtrips() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let order = sample_order();
        let blob = seal_order_with(
            &order,
            &cluster.public_bytes(),
            StaticSecret::from([2u8; 32]),
            [9u8; 12],
        );
        assert_eq!(blob.len(), SEALED_ORDER_LEN);
        assert_eq!(cluster.open(&blob).unwrap(), order);
    }

    #[test]
    fn random_seal_roundtrips() {
        let mut rng = rand::rngs::OsRng;
        let cluster = ClusterKey::from_seed([5u8; 32]);
        let order = sample_order();
        let blob = seal_order(&order, &cluster.public_bytes(), &mut rng);
        assert_eq!(cluster.open(&blob).unwrap(), order);
    }

    #[test]
    fn tampered_ciphertext_fails_authentication() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let mut blob = seal_order_with(
            &sample_order(),
            &cluster.public_bytes(),
            StaticSecret::from([2u8; 32]),
            [9u8; 12],
        );
        let last = blob.len() - 1;
        blob[last] ^= 0xFF; // flip a tag byte
        assert_eq!(cluster.open(&blob), Err(MpcError::SealOpenFailed));
    }

    #[test]
    fn wrong_cluster_key_fails() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        let attacker = ClusterKey::from_seed([42u8; 32]);
        let blob = seal_order_with(
            &sample_order(),
            &cluster.public_bytes(),
            StaticSecret::from([2u8; 32]),
            [9u8; 12],
        );
        assert_eq!(attacker.open(&blob), Err(MpcError::SealOpenFailed));
    }

    #[test]
    fn truncated_blob_is_malformed() {
        let cluster = ClusterKey::from_seed([1u8; 32]);
        assert_eq!(cluster.open(&[0u8; 10]), Err(MpcError::MalformedSealedOrder));
    }
}
