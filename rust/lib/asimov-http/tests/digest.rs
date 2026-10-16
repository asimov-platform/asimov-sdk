// This is free and unencumbered software released into the public domain.

use asimov_http::{ContentDigest, DigestPreference};

#[test]
fn sha256_vectors_work_without_std_features() {
    for (input, expected) in [
        (
            b"".as_slice(),
            "sha-256=:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=:",
        ),
        (
            b"abc".as_slice(),
            "sha-256=:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=:",
        ),
        (
            b"\0\xff\r\n".as_slice(),
            "sha-256=:6UifN/swUenvodyRYATXJ057Y5deMglwiUcmfyOTqb4=:",
        ),
    ] {
        assert_eq!(ContentDigest::new(input).to_field_value(), expected);
    }
    assert_eq!(DigestPreference::new(0).unwrap().value(), 0);
    assert_eq!(DigestPreference::new(10).unwrap().value(), 10);
    assert!(DigestPreference::new(11).is_none());
}
