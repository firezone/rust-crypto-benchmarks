use std::process::ExitCode;

use harness::{OpenError, TAG_LEN};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

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

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("ring", harness::manifest!())
}
