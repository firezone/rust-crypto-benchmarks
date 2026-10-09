use std::process::ExitCode;

use harness::OpenError;

struct ChaCha20Poly1305([u8; 32]);

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self(*key)
    }

    fn seal(&self, nonce: &[u8; 12], aad: &[u8], src: &[u8], dst: &mut [u8]) {
        libcrux_chacha20poly1305::encrypt(&self.0, src, dst, aad, nonce).unwrap();
    }

    fn open(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        src: &[u8],
        dst: &mut [u8],
    ) -> Result<(), OpenError> {
        libcrux_chacha20poly1305::decrypt(&self.0, dst, src, aad, nonce)
            .map(|_| ())
            .map_err(|_| OpenError)
    }
}

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("libcrux", harness::manifest!())
}
