use crate::config::DISTRIBUTION_PUBLIC_KEY;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use ring::signature;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize)]
pub struct LatestRelease {
    pub version: String,
    #[serde(default)]
    pub changelog: String,
    #[serde(default)]
    pub timestamp: u64,
    pub targets: Vec<Target>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub target: String,
    pub url: String,
    pub signature: String,
    pub size: u64,
}
pub const SIGNATURE_SEPARATOR_NONCE: &str = "CraftIPVersion";

pub fn get_bytes_for_signature(hash: &[u8], version: &str) -> Vec<u8> {
    let prefix = format!("{}{}", SIGNATURE_SEPARATOR_NONCE, version);

    // append version
    let mut to_be_checked = Vec::with_capacity(hash.len() + prefix.as_bytes().len());
    to_be_checked.extend_from_slice(hash);
    to_be_checked.extend_from_slice(prefix.as_bytes());

    to_be_checked
}

#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("HTTP Request failed")]
    RequestError(#[from] ureq::Error),
    #[error("Parsing error")]
    ParsingError(ureq::Error),
    #[error("Could not parse version")]
    CouldNotParseVersion,
    #[error("OS architecture not available")]
    TargetNotFound,
    #[error("Could not write/read")]
    IoError(#[from] io::Error),
    #[error("Could not decode base64")]
    Base64DecodeError(#[from] base64::DecodeError),
    #[error("Signature match failed")]
    SignatureMatchFailed,
    #[error("Decompression Failed")]
    DecompressionFailed,
    #[error("Could not replace program")]
    ReplaceFailed(io::Error),
    #[error("Restart failed")]
    RestartFailed(io::Error),
}

pub fn decompress<P: AsRef<Path>>(source: P, dest: P) -> Result<(), UpdaterError> {
    let start = Instant::now();
    let archive = File::open(source)?;
    let archive = BufReader::new(archive);
    let output = File::create(dest)?;
    let mut output = BufWriter::new(output);

    let mut decompressor = liblzma::read::XzDecoder::new(archive);

    let mut buf = [0u8; 1024];
    loop {
        let len = decompressor
            .read(&mut buf)
            .map_err(|_| UpdaterError::DecompressionFailed)?;
        if len == 0 {
            break;
        }
        output.write_all(&buf[..len])?;
    }
    println!(
        "decompression took {}ms",
        (Instant::now() - start).as_millis()
    );
    Ok(())
}

/// verifies the remote signature by computing the downloaded file hash and remote version together, so no downgrade attack
pub fn verify_signature(hash: &[u8], version: &str, signature: &str) -> Result<(), UpdaterError> {
    let to_be_checked = get_bytes_for_signature(hash, version);

    // verify remote signature
    let remote_signature = BASE64_STANDARD.decode(signature)?;

    let public_key =
        signature::UnparsedPublicKey::new(&signature::ED25519, DISTRIBUTION_PUBLIC_KEY);
    public_key
        .verify(to_be_checked.as_slice(), remote_signature.as_slice())
        .map_err(|_| UpdaterError::SignatureMatchFailed)?;
    Ok(())
}
