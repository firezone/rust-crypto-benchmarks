//! libcrux's AEADs only encrypt and decrypt out of place, so the in-place adapters below first
//! copy the input into a scratch buffer, as an in-place caller such as a WireGuard
//! implementation would have to.

use std::cell::RefCell;
use std::process::ExitCode;

use harness::{OpenError, TAG_LEN};
use libcrux_blake2::Blake2sBuilder;
use libcrux_chacha20poly1305::xchacha20_poly1305;

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
        let msg_len = in_out.len() - TAG_LEN;
        scratch.clear();
        scratch.extend_from_slice(&in_out[..msg_len]);
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

struct XChaCha20Poly1305 {
    key: [u8; 32],
    scratch: RefCell<Vec<u8>>,
}

impl harness::XChaCha20Poly1305 for XChaCha20Poly1305 {
    fn new(key: &[u8; 32]) -> Self {
        Self {
            key: *key,
            scratch: RefCell::new(Vec::new()),
        }
    }

    fn seal_in_place(&self, nonce: &[u8; 24], aad: &[u8], in_out: &mut [u8]) {
        let scratch = &mut *self.scratch.borrow_mut();
        let msg_len = in_out.len() - TAG_LEN;
        scratch.clear();
        scratch.extend_from_slice(&in_out[..msg_len]);
        xchacha20_poly1305::encrypt(&self.key, scratch, in_out, aad, nonce).unwrap();
    }
}

struct X25519([u8; 32]);

impl harness::X25519 for X25519 {
    fn new(secret: &[u8; 32]) -> Self {
        Self(*secret)
    }

    fn diffie_hellman(&self, public: &[u8; 32]) -> [u8; 32] {
        let mut shared = [0; 32];
        let res = libcrux_curve25519::ecdh(&mut shared, public, &self.0);
        assert!(res.is_ok(), "all-zero shared secret");
        shared
    }
}

struct Blake2s;

impl harness::Blake2s for Blake2s {
    fn hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = Blake2sBuilder::new_unkeyed().build_const_digest_len::<32>();
        hasher.update(data).unwrap();
        let mut digest = [0; 32];
        hasher.finalize(&mut digest);
        digest
    }
}

fn main() -> ExitCode {
    harness::Suite::new("libcrux", harness::manifest!())
        .chacha20poly1305::<ChaCha20Poly1305>()
        .xchacha20poly1305::<XChaCha20Poly1305>()
        .x25519::<X25519>()
        .blake2s::<Blake2s>()
        .run()
}
