//! Known-answer tests, run before anything is benchmarked.

use crate::{ChaCha20Poly1305, SIZE, TAG_LEN, message};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

fn check(what: &str, actual: &[u8], expected: &[u8]) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    let to_hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Err(format!(
        "{what}: expected {}, got {}",
        to_hex(expected),
        to_hex(actual)
    ))
}

const SUNSCREEN: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

/// RFC 8439 section 2.8.2, plus the tag for the benchmark input (same key, nonce and AAD,
/// message `message(SIZE)`), which exercises the multi-block SIMD paths. That one was generated
/// with OpenSSL.
pub(crate) fn chacha20poly1305<T: ChaCha20Poly1305>() -> Result<(), String> {
    let key: [u8; 32] = hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f")
        .try_into()
        .unwrap();
    let nonce: [u8; 12] = hex("07000000 4041424344454647").try_into().unwrap();
    let aad = hex("50515253c0c1c2c3c4c5c6c7");
    let cipher = T::new(&key);

    let expected_ct = hex(
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6
         3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36
         92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc
         3ff4def08e4b7a9de576d26586cec64b6116
         1ae10b594f09e26a7e902ecbd0600691",
    );
    let mut sealed = vec![0; SUNSCREEN.len() + TAG_LEN];
    cipher.seal(&nonce, &aad, SUNSCREEN, &mut sealed);
    check("RFC 8439 seal", &sealed, &expected_ct)?;

    let mut opened = vec![0; expected_ct.len()];
    cipher
        .open(&nonce, &aad, &expected_ct, &mut opened)
        .map_err(|_| "RFC 8439 open: rejected a valid ciphertext".to_owned())?;
    check("RFC 8439 open", &opened[..SUNSCREEN.len()], SUNSCREEN)?;

    let mut tampered = expected_ct;
    tampered[0] ^= 1;
    if cipher.open(&nonce, &aad, &tampered, &mut opened).is_ok() {
        return Err("open accepted a tampered ciphertext".to_owned());
    }

    let plaintext = message(SIZE);
    let mut sealed = vec![0; SIZE + TAG_LEN];
    cipher.seal(&nonce, &aad, &plaintext, &mut sealed);
    check(
        "1280-byte seal tag",
        &sealed[SIZE..],
        &hex("5c9b226aada83f5fe6bf25a5fd15e86b"),
    )?;
    let mut opened = vec![0; SIZE + TAG_LEN];
    cipher
        .open(&nonce, &aad, &sealed, &mut opened)
        .map_err(|_| "1280-byte open: rejected its own ciphertext".to_owned())?;
    check("1280-byte roundtrip", &opened[..SIZE], &plaintext)
}
