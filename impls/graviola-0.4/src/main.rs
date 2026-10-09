use std::process::ExitCode;

use harness::{OpenError, TAG_LEN};

struct ChaCha20Poly1305(graviola::aead::ChaCha20Poly1305);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(graviola::aead::ChaCha20Poly1305::new(*key))
    }

    fn seal(&self, nonce: &[u8; 12], aad: &[u8], src: &[u8], dst: &mut [u8]) {
        let (msg, tag) = dst.split_at_mut(src.len());
        msg.copy_from_slice(src);
        self.0.encrypt(nonce, aad, msg, tag.try_into().unwrap());
    }

    fn open(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        src: &[u8],
        dst: &mut [u8],
    ) -> Result<(), OpenError> {
        let (ct, tag) = src.split_at(src.len() - TAG_LEN);
        let msg = &mut dst[..ct.len()];
        msg.copy_from_slice(ct);
        self.0.decrypt(nonce, aad, msg, tag).map_err(|_| OpenError)
    }
}

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("graviola", harness::manifest!())
}
