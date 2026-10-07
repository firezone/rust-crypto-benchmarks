//! `chacha20poly1305` 0.10, built on the `aead` 0.5 traits.

use std::process::ExitCode;

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{Key, Nonce, Tag};
use harness::{OpenError, TAG_LEN};

struct ChaCha20Poly1305(chacha20poly1305::ChaCha20Poly1305);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(chacha20poly1305::ChaCha20Poly1305::new(Key::from_slice(
            key,
        )))
    }

    fn seal_in_place(&self, nonce: &[u8; 12], aad: &[u8], in_out: &mut [u8]) {
        let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
        let t = self
            .0
            .encrypt_in_place_detached(Nonce::from_slice(nonce), aad, msg)
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
        self.0
            .decrypt_in_place_detached(Nonce::from_slice(nonce), aad, msg, Tag::from_slice(tag))
            .map_err(|_| OpenError)
    }
}

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("RustCrypto", harness::manifest!())
}
