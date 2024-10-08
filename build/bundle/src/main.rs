mod config;
mod updater_proto;

use crate::config::UPDATE_URL;
use crate::updater_proto::{
    decompress, get_bytes_for_signature, verify_signature, LatestRelease, Target, UpdaterError,
};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use clap::Parser;
use ring::digest::{Context, Digest, SHA512};
use ring::signature;
use rpassword::read_password;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};
use std::{env, fs};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long)]
    input: Option<String>,
    #[arg(short, long)]
    output: Option<String>,
    #[arg(short, long)]
    ver: Option<String>,

    #[arg(short, long, help = "test remote json")]
    test_staging: bool,
}

fn main() {
    let args = Args::parse();

    if args.test_staging {
        let json_url = format!("{}.staging.json", UPDATE_URL);
        verify_release_json(json_url.as_str());
        println!("done!");
        return;
    }

    let url_prefix = "https://update.craftip.net/update/v1/binaries/";

    let (input, version) = (
        args.input.as_ref().unwrap().as_str(),
        args.ver.as_ref().unwrap().as_str(),
    );

    let _output = format!("{}/binaries", args.output.as_ref().unwrap());
    let _output_latest = format!("{}/latest.json.staging.json", args.output.as_ref().unwrap());
    let (output, output_latest) = (_output.as_str(), _output_latest.as_str());

    print!("Type in private key: ");
    std::io::stdout().flush().unwrap();
    let key = read_password().unwrap();

    fs::create_dir_all(output).unwrap();

    let targets = [
        "x86_64-pc-windows-msvc",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ];
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut release = LatestRelease {
        version: version.to_string(),
        changelog: "".to_string(),
        timestamp,
        targets: vec![],
    };
    let temp_folder = tempfile::TempDir::new().unwrap();

    for target in targets {
        let executable = Path::new(input).join(target);
        println!("Compressing {:?}...", executable);
        let compressed_exe_name = format!("{}-{}-{}.xz", target, version, timestamp);
        let compressed_exe = Path::new(output).join(compressed_exe_name.as_str());
        compress(executable.clone(), compressed_exe.clone()).unwrap();
        println!("Signing {:?}", compressed_exe);
        let signature = sign_file(compressed_exe.clone(), key.as_str());

        let size = File::open(compressed_exe.clone())
            .unwrap()
            .metadata()
            .unwrap()
            .len();
        let url = format!("{}{}", url_prefix, compressed_exe_name.as_str());
        let json_target = Target {
            url,
            target: target.to_string(),
            signature,
            size,
        };

        verify_signature_of_file(compressed_exe.clone(), &json_target, version).unwrap();

        let decompression_test = temp_folder.path().join(target);

        decompress(compressed_exe.clone(), decompression_test.clone()).unwrap();
        let hash = hash_file(decompression_test.clone());
        let original_hash = hash_file(executable.clone());
        assert_eq!(
            hash.as_ref(),
            original_hash.as_ref(),
            "compression failed somehow. output hashes are not the same"
        );

        release.targets.push(json_target);
    }
    println!("{:?}", release);
    let json = serde_json::to_string(&release).unwrap();
    File::create(output_latest)
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();
}

fn verify_release_json(url: &str) {
    let release = ureq::get(url)
        .call()
        .unwrap()
        .into_body()
        .read_json::<LatestRelease>()
        .unwrap();
    let temp_folder = tempfile::TempDir::new().unwrap();
    for target in release.targets {
        let resp = ureq::get(&target.url)
            .call()
            .unwrap()
            .into_body()
            .read_to_vec()
            .unwrap();

        let archive = temp_folder.path().join(format!("{}.xz", target.target));
        File::create(archive.clone())
            .unwrap()
            .write_all(resp.as_slice())
            .unwrap();

        // verify if signature is matching
        verify_signature_of_file(archive.clone(), &target, release.version.as_str()).unwrap();
    }
}

fn verify_signature_of_file(
    archive: PathBuf,
    target: &Target,
    version: &str,
) -> Result<(), UpdaterError> {
    println!("Verifying...");
    let file_length = fs::read(archive.clone()).unwrap().len();
    assert_eq!(file_length, target.size as usize, "Size is not correct");
    assert_ne!(file_length, 0, "File length should not be zero");

    let hash = hash_file(archive.clone());
    crate::updater_proto::verify_signature(hash.as_ref(), version, target.signature.as_str())
        .expect(
        "Something went wrong. Could not verify signature. Are Public and private keys matching",
    );
    Ok(())
}

fn sign_file<P: AsRef<Path>>(file: P, key: &str) -> String {
    let hash = hash_file(file);

    let version = "0.0.1";
    let bytes_for_sig = get_bytes_for_signature(hash.as_ref(), version);

    let key = BASE64_STANDARD.decode(key).unwrap();
    let key = signature::Ed25519KeyPair::from_pkcs8_maybe_unchecked(&key).unwrap();
    let signature = key.sign(&bytes_for_sig);

    BASE64_STANDARD.encode(&signature)
}

pub fn compress<P: AsRef<Path>>(source: P, dest: P) -> Result<(), UpdaterError> {
    let start = Instant::now();
    let input = File::open(source)?;
    let input = BufReader::new(input);
    let output = File::create(dest)?;
    let mut output = BufWriter::new(output);

    let mut compressor = liblzma::read::XzEncoder::new(input, 9);

    let mut buf = [0u8; 1024];
    loop {
        let len = compressor.read(&mut buf).unwrap();
        if len == 0 {
            break;
        }
        output.write_all(&buf[..len])?;
    }
    println!(
        "compression took {}ms",
        (Instant::now() - start).as_millis()
    );
    Ok(())
}

fn hash_file<P: AsRef<Path>>(file: P) -> Digest {
    let file = fs::read(file).unwrap();
    let mut hash = Context::new(&SHA512);
    hash.update(file.as_slice());
    hash.finish()
}
