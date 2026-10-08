use std::process::ExitCode;

use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use harness::OpenError;

struct ChaCha20Poly1305(LessSafeKey);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, key).unwrap(),
        ))
    }

    fn seal(&self, nonce: &[u8; 12], aad: &[u8], src: &[u8], dst: &mut [u8]) {
        let (msg, tag) = dst.split_at_mut(src.len());
        msg.copy_from_slice(src);
        let t = self
            .0
            .seal_in_place_separate_tag(Nonce::assume_unique_for_key(*nonce), Aad::from(aad), msg)
            .unwrap();
        tag.copy_from_slice(t.as_ref());
    }

    fn open(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        src: &[u8],
        dst: &mut [u8],
    ) -> Result<(), OpenError> {
        dst.copy_from_slice(src);
        self.0
            .open_in_place(Nonce::assume_unique_for_key(*nonce), Aad::from(aad), dst)
            .map(|_| ())
            .map_err(|_| OpenError)
    }
}

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("aws-lc-rs", harness::manifest!())
}
