//! libcrux only encrypts and decrypts out of place, so the in-place adapter first copies the
//! input into a scratch buffer, as an in-place caller such as a WireGuard implementation would
//! have to.

use std::cell::RefCell;
use std::process::ExitCode;

use harness::{OpenError, TAG_LEN};

struct ChaCha20Poly1305 {
    key: [u8; 32],
    scratch: RefCell<Vec<u8>>,
}

impl harness::ChaCha20Poly1305 for ChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self {
            key: *key,
            scratch: RefCell::new(Vec::new()),
        }
    }

    fn seal_in_place(&self, nonce: &[u8; 12], aad: &[u8], in_out: &mut [u8]) {
        let scratch = &mut *self.scratch.borrow_mut();
        scratch.clear();
        scratch.extend_from_slice(&in_out[..in_out.len() - TAG_LEN]);
        libcrux_chacha20poly1305::encrypt(&self.key, scratch, in_out, aad, nonce).unwrap();
    }

    fn open_in_place(
        &self,
        nonce: &[u8; 12],
        aad: &[u8],
        in_out: &mut [u8],
    ) -> Result<(), OpenError> {
        let scratch = &mut *self.scratch.borrow_mut();
        scratch.clear();
        scratch.extend_from_slice(in_out);
        libcrux_chacha20poly1305::decrypt(&self.key, in_out, scratch, aad, nonce)
            .map(|_| ())
            .map_err(|_| OpenError)
    }
}

fn main() -> ExitCode {
    harness::run::<ChaCha20Poly1305>("libcrux", harness::manifest!())
}
