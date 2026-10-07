use std::process::ExitCode;

use harness::{OpenError, TAG_LEN};

struct ChaCha20Poly1305(graviola::aead::ChaCha20Poly1305);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(graviola::aead::ChaCha20Poly1305::new(*key))
    }

    fn seal_in_place(&self, nonce: &[u8; 12], aad: &[u8], in_out: &mut [u8]) {
        let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
        self.0.encrypt(nonce, aad, msg, tag.try_into().unwrap());
    }

    fn open_in_place(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<(), OpenError> {
        let (msg, tag) = in_out.split_at_mut(in_out.len() - TAG_LEN);
        self.0.decrypt(nonce, aad, msg, tag).map_err(|_| OpenError)
    }
}

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("graviola", harness::manifest!())
}
