//! Artifact signing (ADR-0005 follow-up, production gate "attestation
//! verified before activation").
//!
//! `usai build --sign <key>` writes `signature.json` next to the artifact:
//! the SHA-256 of every file that the runtime will read (manifest, bundle,
//! migrations, source map, and the precompiled image — native code), and an
//! Ed25519 signature over that canonical list. A runtime started with
//! `--require-signature <public key>` (or `USAI_REQUIRE_SIGNATURE`) refuses,
//! before anything listens, an artifact without a signature, with a
//! signature by another key, or with any listed file changed — and an
//! artifact that carries a file the signature does not cover.

use std::collections::BTreeMap;
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SIGNATURE_FILE: &str = "signature.json";

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("signing key: {0}")]
    Key(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSignature {
    pub algorithm: String,
    pub public_key: String,
    /// Artifact-relative path → SHA-256 hex.
    pub files: BTreeMap<String, String>,
    pub signature: String,
}

/// A 32-byte Ed25519 seed, hex, in a file (`usai keygen`).
pub fn load_signing_key(path: &Path) -> Result<SigningKey, SigningError> {
    let text = std::fs::read_to_string(path)?;
    let bytes = hex::decode(text.trim())
        .map_err(|e| SigningError::Key(format!("{}: not hex: {e}", path.display())))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| SigningError::Key(format!("{}: expected 32 bytes", path.display())))?;
    Ok(SigningKey::from_bytes(&seed))
}

pub fn generate_key() -> (SigningKey, String) {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("operating system entropy");
    let key = SigningKey::from_bytes(&seed);
    let public = hex::encode(key.verifying_key().to_bytes());
    (key, public)
}

pub fn parse_public_key(hex_text: &str) -> Result<VerifyingKey, SigningError> {
    let bytes = hex::decode(hex_text.trim())
        .map_err(|e| SigningError::Key(format!("public key is not hex: {e}")))?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| SigningError::Key("public key must be 32 bytes".into()))?;
    VerifyingKey::from_bytes(&array).map_err(|e| SigningError::Key(e.to_string()))
}

/// Every file in the artifact except the signature itself and the build's
/// own bookkeeping (`inputs.json` describes the source tree, not the
/// artifact).
fn artifact_files(dir: &Path) -> Result<Vec<String>, SigningError> {
    let mut out = Vec::new();
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(base, &path, out)?;
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel != SIGNATURE_FILE && rel != "inputs.json" {
                    out.push(rel);
                }
            }
        }
        Ok(())
    }
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn canonical(files: &BTreeMap<String, String>) -> Vec<u8> {
    // Deterministic: sorted map, no whitespace.
    serde_json::to_vec(files).expect("map serializes")
}

pub fn sign_artifact(dir: &Path, key: &SigningKey) -> Result<ArtifactSignature, SigningError> {
    let mut files = BTreeMap::new();
    for rel in artifact_files(dir)? {
        let bytes = std::fs::read(dir.join(&rel))?;
        files.insert(rel, hex::encode(Sha256::digest(&bytes)));
    }
    let signature = key.sign(&canonical(&files));
    let record = ArtifactSignature {
        algorithm: "ed25519".into(),
        public_key: hex::encode(key.verifying_key().to_bytes()),
        files,
        signature: hex::encode(signature.to_bytes()),
    };
    std::fs::write(
        dir.join(SIGNATURE_FILE),
        serde_json::to_vec_pretty(&record).expect("record serializes"),
    )?;
    Ok(record)
}

/// Verifies `dir` against `trusted` keys: signature present, key trusted,
/// signature valid, every listed file unchanged, no unlisted file.
pub fn verify_artifact(
    dir: &Path,
    trusted: &[VerifyingKey],
) -> Result<ArtifactSignature, SigningError> {
    let path = dir.join(SIGNATURE_FILE);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        SigningError::Invalid(format!(
            "{} has no {SIGNATURE_FILE}; this runtime requires signed artifacts (`usai build --sign <key>`)",
            dir.display()
        ))
    })?;
    let record: ArtifactSignature = serde_json::from_str(&text)
        .map_err(|e| SigningError::Invalid(format!("{SIGNATURE_FILE}: {e}")))?;
    if record.algorithm != "ed25519" {
        return Err(SigningError::Invalid(format!(
            "{SIGNATURE_FILE}: unsupported algorithm {}",
            record.algorithm
        )));
    }
    let key = parse_public_key(&record.public_key)?;
    if !trusted.iter().any(|t| t == &key) {
        return Err(SigningError::Invalid(format!(
            "artifact is signed by {}, which this runtime does not trust",
            &record.public_key[..16]
        )));
    }
    let signature_bytes = hex::decode(&record.signature).map_err(|e| {
        SigningError::Invalid(format!("{SIGNATURE_FILE}: signature is not hex: {e}"))
    })?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|e| SigningError::Invalid(format!("{SIGNATURE_FILE}: {e}")))?;
    key.verify(&canonical(&record.files), &signature)
        .map_err(|_| SigningError::Invalid("artifact signature does not verify".into()))?;
    for (rel, expected) in &record.files {
        let bytes = std::fs::read(dir.join(rel)).map_err(|_| {
            SigningError::Invalid(format!("signed file {rel} is missing from the artifact"))
        })?;
        let actual = hex::encode(Sha256::digest(&bytes));
        if &actual != expected {
            return Err(SigningError::Invalid(format!(
                "{rel} does not match its signed digest (the artifact was modified after signing)"
            )));
        }
    }
    for rel in artifact_files(dir)? {
        if !record.files.contains_key(&rel) {
            return Err(SigningError::Invalid(format!(
                "{rel} is present but not covered by the signature"
            )));
        }
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_verifies_and_refuses_tampering() {
        let dir = std::env::temp_dir().join(format!("usai-sign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        std::fs::write(dir.join("manifest.json"), b"{}").unwrap();
        std::fs::write(dir.join("app.js"), b"1").unwrap();
        std::fs::write(dir.join("cache/image.cwasm"), b"native").unwrap();
        std::fs::write(dir.join("inputs.json"), b"[]").unwrap();
        let (key, public) = generate_key();
        let record = sign_artifact(&dir, &key).unwrap();
        assert_eq!(
            record.files.len(),
            3,
            "inputs.json is not part of the artifact"
        );
        let trusted = vec![parse_public_key(&public).unwrap()];
        verify_artifact(&dir, &trusted).unwrap();
        // Another key: refused.
        let (_, other) = generate_key();
        let err = verify_artifact(&dir, &[parse_public_key(&other).unwrap()]).unwrap_err();
        assert!(err.to_string().contains("does not trust"), "{err}");
        // A changed native image: refused.
        std::fs::write(dir.join("cache/image.cwasm"), b"evil").unwrap();
        let err = verify_artifact(&dir, &trusted).unwrap_err();
        assert!(err.to_string().contains("modified after signing"), "{err}");
        std::fs::write(dir.join("cache/image.cwasm"), b"native").unwrap();
        // An extra file the signature does not cover: refused.
        std::fs::write(dir.join("extra.js"), b"x").unwrap();
        let err = verify_artifact(&dir, &trusted).unwrap_err();
        assert!(err.to_string().contains("not covered"), "{err}");
        std::fs::remove_file(dir.join("extra.js")).unwrap();
        // No signature at all: refused with the fix.
        std::fs::remove_file(dir.join(SIGNATURE_FILE)).unwrap();
        let err = verify_artifact(&dir, &trusted).unwrap_err();
        assert!(err.to_string().contains("usai build --sign"), "{err}");
    }
}
