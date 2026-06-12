use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub(super) fn normalize_language_key(language: &str) -> String {
    language.trim().to_ascii_lowercase()
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub(super) fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
