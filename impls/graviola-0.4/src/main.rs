//! `graviola` has no BLAKE2s.

use std::process::ExitCode;

use graviola::key_agreement::x25519::{PublicKey, StaticPrivateKey};
use harness::{OpenError, TAG_LEN};

fn split(in_out: &mut [u8]) -> (&mut [u8], &mut [u8; TAG_LEN]) {
    let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
    (msg, tag.try_into().unwrap())
}

struct ChaCha20Poly1305(graviola::aead::ChaCha20Poly1305);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(graviola::aead::ChaCha20Poly1305::new(*key))
    }

    fn seal_in_place(&self, nonce: &[u8; 12], aad: &[u8], in_out: &mut [u8]) {
        let (msg, tag) = split(in_out);
        self.0.encrypt(nonce, aad, msg, tag);
    }

    fn open_in_place(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<(), OpenError> {
        let (msg, tag) = split(in_out);
        self.0.decrypt(nonce, aad, msg, tag).map_err(|_| OpenError)
    }
}

struct XChaCha20Poly1305(graviola::aead::XChaCha20Poly1305);

impl harness::XChaCha20Poly1305 for XChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(graviola::aead::XChaCha20Poly1305::new(*key))
    }

    fn seal_in_place(&self, nonce: &[u8; 24], aad: &[u8], in_out: &mut [u8]) {
        let (msg, tag) = split(in_out);
        self.0.encrypt(nonce, aad, msg, tag);
    }
}

struct X25519(StaticPrivateKey);

impl harness::X25519 for X25519 {
    fn new(secret: &[u8; 32]) -> Self {
        Self(StaticPrivateKey::from_array(secret))
    }

    fn diffie_hellman(&self, public: &[u8; 32]) -> [u8; 32] {
        self.0
            .diffie_hellman(&PublicKey::from_array(public))
            .unwrap()
            .as_bytes()
    }
}

fn main() -> ExitCode {
    harness::Suite::new("graviola", harness::manifest!())
        .chacha20poly1305::<ChaCha20Poly1305>()
        .xchacha20poly1305::<XChaCha20Poly1305>()
        .x25519::<X25519>()
        .run()
}
