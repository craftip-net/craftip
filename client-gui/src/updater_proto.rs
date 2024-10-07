use std::fs::File;
use std::io;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize)]
pub struct LatestRelease {
    pub version: String,
    pub changelog: String,
    pub targets: Vec<Target>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub name: String,
    pub url: String,
    pub target: String,
    pub signature: String,
    pub size: u64,
}
pub const SIGNATURE_SEPARATOR_NONCE: &str = "CraftIPVersion";
pub const DISTRIBUTION_PUBLIC_KEY: [u8; 32] = [0xac, 0x53, 0xd0, 0x20, 0x59, 0x61, 0x92, 0x11, 0x26, 0x74, 0x38, 0x95, 0x47, 0xe2, 0xff, 0x8a, 0x11, 0x62, 0x3c, 0x2c, 0x14, 0xd9, 0xf5, 0xfb, 0x14, 0x7d, 0x68, 0xf8, 0x8d, 0xf8, 0x6b, 0x2f];

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
    RestartFailed
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
        let len = decompressor.read(&mut buf).map_err(|_|UpdaterError::DecompressionFailed)?;
        if len == 0 {
            break;
        }
        output.write_all(&buf[..len])?;
    }
    println!("decompression took {}ms", (Instant::now() - start).as_millis());
    Ok(())
}