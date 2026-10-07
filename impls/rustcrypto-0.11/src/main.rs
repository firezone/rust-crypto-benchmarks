//! `chacha20poly1305` 0.11, built on the `aead` 0.6 traits.

use std::process::ExitCode;

use chacha20poly1305::aead::inout::InOutBuf;
use chacha20poly1305::{AeadInOut, Key, KeyInit, Nonce, Tag};
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

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("RustCrypto", harness::manifest!())
}
