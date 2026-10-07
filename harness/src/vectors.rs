//! Known-answer tests, run before anything is benchmarked.

use crate::{Blake2s, ChaCha20Poly1305, TAG_LEN, X25519, XChaCha20Poly1305, input};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

fn arr<const N: usize>(s: &str) -> [u8; N] {
    hex(s).try_into().expect("vector has the right length")
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
const AEAD_KEY: &str = "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f";
const AEAD_AAD: &str = "50515253c0c1c2c3c4c5c6c7";

pub(crate) const X25519_ALICE_SECRET: [u8; 32] = [
    0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d, 0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2, 0x66, 0x45,
    0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a, 0xb1, 0x77, 0xfb, 0xa5, 0x1d, 0xb9, 0x2c, 0x2a,
];
pub(crate) const X25519_BOB_PUBLIC: [u8; 32] = [
    0xde, 0x9e, 0xdb, 0x7d, 0x7b, 0x7d, 0xc1, 0xb4, 0xd3, 0x5b, 0x61, 0xc2, 0xec, 0xe4, 0x35, 0x37,
    0x3f, 0x83, 0x43, 0xc8, 0x5b, 0x78, 0x67, 0x4d, 0xad, 0xfc, 0x7e, 0x14, 0x6f, 0x88, 0x2b, 0x4f,
];

/// RFC 8439 section 2.8.2, plus tags for the benchmark inputs (same key, nonce and AAD, message
/// `input(len)`) that exercise the multi-block SIMD paths. Those were generated with OpenSSL.
pub(crate) fn chacha20poly1305<T: ChaCha20Poly1305>() -> Result<(), String> {
    let key = arr::<32>(AEAD_KEY);
    let nonce = arr::<12>("07000000 4041424344454647");
    let aad = hex(AEAD_AAD);
    let cipher = T::new(&key);

    let expected_ct = hex(
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6
         3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36
         92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc
         3ff4def08e4b7a9de576d26586cec64b6116
         1ae10b594f09e26a7e902ecbd0600691",
    );
    let mut buf = SUNSCREEN.to_vec();
    buf.extend_from_slice(&[0; TAG_LEN]);
    cipher.seal_in_place(&nonce, &aad, &mut buf);
    check("RFC 8439 seal", &buf, &expected_ct)?;

    let mut buf = expected_ct.clone();
    cipher
        .open_in_place(&nonce, &aad, &mut buf)
        .map_err(|_| "RFC 8439 open: rejected a valid ciphertext".to_owned())?;
    check("RFC 8439 open", &buf[..SUNSCREEN.len()], SUNSCREEN)?;

    let mut buf = expected_ct;
    buf[0] ^= 1;
    if cipher.open_in_place(&nonce, &aad, &mut buf).is_ok() {
        return Err("open accepted a tampered ciphertext".to_owned());
    }

    for (len, tag) in [
        (32, "2f306d8cfe2bb7f23c5cc394fff314fa"),
        (64, "39b0e033cfc353dcd39b633167441401"),
        (1280, "5c9b226aada83f5fe6bf25a5fd15e86b"),
        (1420, "e1111786e7cd307ee7f0f9003f7f932f"),
    ] {
        let plaintext = input(len);
        let mut buf = plaintext.clone();
        buf.extend_from_slice(&[0; TAG_LEN]);
        cipher.seal_in_place(&nonce, &aad, &mut buf);
        check(&format!("{len}-byte seal tag"), &buf[len..], &hex(tag))?;
        cipher
            .open_in_place(&nonce, &aad, &mut buf)
            .map_err(|_| format!("{len}-byte open: rejected its own ciphertext"))?;
        check(&format!("{len}-byte roundtrip"), &buf[..len], &plaintext)?;
    }
    Ok(())
}

/// draft-irtf-cfrg-xchacha-03 section A.3.1.
pub(crate) fn xchacha20poly1305<T: XChaCha20Poly1305>() -> Result<(), String> {
    let cipher = T::new(&arr::<32>(AEAD_KEY));
    let nonce = arr::<24>("404142434445464748494a4b4c4d4e4f5051525354555657");
    let expected = hex(
        "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb
         731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452
         2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9
         21f9664c97637da9768812f615c68b13b52e
         c0875924c1c7987947deafd8780acf49",
    );
    let mut buf = SUNSCREEN.to_vec();
    buf.extend_from_slice(&[0; TAG_LEN]);
    cipher.seal_in_place(&nonce, &hex(AEAD_AAD), &mut buf);
    check("draft-irtf-cfrg-xchacha-03 A.3.1 seal", &buf, &expected)
}

/// RFC 7748 sections 5.2 (first vector) and 6.1.
pub(crate) fn x25519<T: X25519>() -> Result<(), String> {
    let out = T::new(&arr(
        "a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4",
    ))
    .diffie_hellman(&arr(
        "e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c",
    ));
    check(
        "RFC 7748 5.2",
        &out,
        &hex("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"),
    )?;

    let shared = hex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
    let alice = T::new(&X25519_ALICE_SECRET).diffie_hellman(&X25519_BOB_PUBLIC);
    check("RFC 7748 6.1 (Alice)", &alice, &shared)?;
    let bob = T::new(&arr(
        "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb",
    ))
    .diffie_hellman(&arr(
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a",
    ));
    check("RFC 7748 6.1 (Bob)", &bob, &shared)
}

/// RFC 7693 appendix B, the empty message, and digests of the benchmark inputs (generated with
/// Python's `hashlib`).
pub(crate) fn blake2s<T: Blake2s>() -> Result<(), String> {
    check(
        "RFC 7693 appendix B",
        &T::hash(b"abc"),
        &hex("508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982"),
    )?;
    check(
        "empty message",
        &T::hash(b""),
        &hex("69217a3079908094e11121d042354a7c1f55b6482ca1a51e1b250dfd1ed0eef9"),
    )?;
    for (len, digest) in [
        (
            32,
            "05825607d7fdf2d82ef4c3c8c2aea961ad98d60edff7d018983e21204c0d93d1",
        ),
        (
            64,
            "56f34e8b96557e90c1f24b52d0c89d51086acf1b00f634cf1dde9233b8eaaa3e",
        ),
        (
            1280,
            "7dbaf94277fcb7a8330e491bc980d3c97bc0198b0d28ff951eff3951bbb6c88f",
        ),
        (
            1420,
            "7b7660ad5190a3921e5fc7cc8a3aef48511e0a9ff69e2cd747cf68453afc109f",
        ),
    ] {
        check(
            &format!("{len}-byte input"),
            &T::hash(&input(len)),
            &hex(digest),
        )?;
    }
    Ok(())
}
