//! RustCrypto as of the `aead` 0.6 / `digest` 0.11 generation: `chacha20poly1305` 0.11,
//! `x25519-dalek` 3 and `blake2` 0.11.

use std::process::ExitCode;

use blake2::{Blake2s256, Digest};
use chacha20poly1305::aead::inout::InOutBuf;
use chacha20poly1305::{AeadInOut, Key, KeyInit, Nonce, Tag, XNonce};
use harness::{OpenError, TAG_LEN};

struct ChaCha20Poly1305(chacha20poly1305::ChaCha20Poly1305);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(chacha20poly1305::ChaCha20Poly1305::new(&Key::from(*key)))
    }

    fn seal_in_place(&self, nonce: &[u8; 12], aad: &[u8], in_out: &mut [u8]) {
        let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
        let t = self
            .0
            .encrypt_inout_detached(&Nonce::from(*nonce), aad, InOutBuf::from(msg))
            .unwrap();
        tag.copy_from_slice(&t);
    }

    fn open_in_place(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<(), OpenError> {
        let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
        let tag = <&Tag>::try_from(&*tag).unwrap();
        self.0
            .decrypt_inout_detached(&Nonce::from(*nonce), aad, InOutBuf::from(msg), tag)
            .map_err(|_| OpenError)
    }
}

struct XChaCha20Poly1305(chacha20poly1305::XChaCha20Poly1305);

impl harness::XChaCha20Poly1305 for XChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(chacha20poly1305::XChaCha20Poly1305::new(&Key::from(*key)))
    }

    fn seal_in_place(&self, nonce: &[u8; 24], aad: &[u8], in_out: &mut [u8]) {
        let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
        let t = self
            .0
            .encrypt_inout_detached(&XNonce::from(*nonce), aad, InOutBuf::from(msg))
            .unwrap();
        tag.copy_from_slice(&t);
    }
}

struct X25519(x25519_dalek::StaticSecret);

impl harness::X25519 for X25519 {
    fn new(secret: &[u8; 32]) -> Self {
        Self(x25519_dalek::StaticSecret::from(*secret))
    }

    fn diffie_hellman(&self, public: &[u8; 32]) -> [u8; 32] {
        self.0
            .diffie_hellman(&x25519_dalek::PublicKey::from(*public))
            .to_bytes()
    }
}

struct Blake2s;

impl harness::Blake2s for Blake2s {
    fn hash(data: &[u8]) -> [u8; 32] {
        Blake2s256::digest(data).into()
    }
}

fn main() -> ExitCode {
    harness::Suite::new("RustCrypto", harness::manifest!())
        .chacha20poly1305::<ChaCha20Poly1305>()
        .xchacha20poly1305::<XChaCha20Poly1305>()
        .x25519::<X25519>()
        .blake2s::<Blake2s>()
        .run()
}
