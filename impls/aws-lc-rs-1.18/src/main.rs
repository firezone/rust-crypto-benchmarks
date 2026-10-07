//! `aws-lc-rs` exposes neither BLAKE2s nor XChaCha20-Poly1305.

use std::process::ExitCode;

use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::agreement::{self, PrivateKey, UnparsedPublicKey};
use harness::{OpenError, TAG_LEN};

struct ChaCha20Poly1305(LessSafeKey);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, key).unwrap(),
        ))
    }

    fn seal_in_place(&self, nonce: &[u8; 12], aad: &[u8], in_out: &mut [u8]) {
        let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
        let t = self
            .0
            .seal_in_place_separate_tag(Nonce::assume_unique_for_key(*nonce), Aad::from(aad), msg)
            .unwrap();
        tag.copy_from_slice(t.as_ref());
    }

    fn open_in_place(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<(), OpenError> {
        self.0
            .open_in_place(Nonce::assume_unique_for_key(*nonce), Aad::from(aad), in_out)
            .map(|_| ())
            .map_err(|_| OpenError)
    }
}

struct X25519(PrivateKey);

impl harness::X25519 for X25519 {
    fn new(secret: &[u8; 32]) -> Self {
        Self(PrivateKey::from_private_key(&agreement::X25519, secret).unwrap())
    }

    fn diffie_hellman(&self, public: &[u8; 32]) -> [u8; 32] {
        let public = UnparsedPublicKey::new(&agreement::X25519, public);
        agreement::agree(&self.0, public, (), |k| Ok(k.try_into().unwrap())).unwrap()
    }
}

fn main() -> ExitCode {
    harness::Suite::new("aws-lc-rs", harness::manifest!())
        .chacha20poly1305::<ChaCha20Poly1305>()
        .x25519::<X25519>()
        .run()
}
