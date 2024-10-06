use anyhow::{bail, format_err, Result};
use semver::Version;
use shared::config::UPDATE_URL;
use std::env::consts::EXE_SUFFIX;
use std::fs::File;
use std::io::{BufRead, Write};
use std::{env, fs, io, process};
use std::path::Path;
use base64::prelude::*;
use image::EncodableLayout;
use reqwest::blocking::Client;
use ring::digest::{Context, SHA512};
use ring::signature;
use crate::updater_proto::{DISTRIBUTION_PUBLIC_KEY, get_bytes_for_signature, LatestRelease};

// https://github.com/lichess-org/fishnet/blob/90f12cd532a43002a276302738f916210a2d526d/src/main.rs
#[cfg(unix)]
fn exec(command: &mut process::Command) -> io::Error {
    use std::os::unix::process::CommandExt as _;
    // Completely replace the current process image. If successful, execution
    // of the current process stops here.
    command.exec()
}

#[cfg(windows)]
fn exec(command: &mut process::Command) -> io::Error {
    use std::os::windows::process::CommandExt as _;
    // No equivalent for Unix exec() exists. So create a new independent
    // console instead and terminate the current one:
    // https://docs.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
    let create_new_console = 0x0000_0010;
    match command.creation_flags(create_new_console).spawn() {
        Ok(_) => process::exit(0),
        Err(err) => return err,
    }
}

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct Updater {
    release: Option<LatestRelease>,
}
impl Updater {
    pub fn check_for_update(&mut self) -> Result<bool> {
        //set_ssl_vars!();
        let api_url = UPDATE_URL.to_string();

        let resp = Client::new().get(&api_url).send()?;
        if !resp.status().is_success() {
            bail!(
                "api request failed with status: {:?} - for: {:?}",
                resp.status(),
                api_url
            )
        }
        println!("hello from the updater");
        let release = resp.json::<LatestRelease>()?;

        let new_version = Version::parse(CURRENT_VERSION)? < Version::parse(&release.version)?;

        if new_version {
            println!("New version available: v{}", release.version);
            self.release = Some(release);
        } else {
            println!("up to date");
        }

        Ok(new_version)
    }
    pub fn update(&mut self) -> Result<()> {
        println!(
            "Checking target-arch... {}",
            current_platform::CURRENT_PLATFORM
        );
        println!("Checking current version... v{}", CURRENT_VERSION);

        println!("Checking latest released version... ");

        let release = self.release.as_ref().unwrap();
        println!("v{:?}", release);

        let target_asset = release
            .targets
            .iter()
            .find(|t| t.target == current_platform::CURRENT_PLATFORM)
            .ok_or_else(|| {
                format_err!(
                    "No release found for target: {}",
                    current_platform::CURRENT_PLATFORM
                )
            })?;


        let tmp_archive_dir = tempfile::TempDir::new()?;
        let zip_file = tmp_archive_dir.path().join(&target_asset.name);

        println!("Downloading...");

        let resp = reqwest::blocking::get(&target_asset.url).expect("request failed");
        let mut out = File::create(&zip_file).expect("failed to create file");

        let mut hash = Context::new(&SHA512);
        let mut src = io::BufReader::new(resp);
        loop {
            let n = {
                let buf = src.fill_buf()?;
                hash.update(buf);
                out.write_all(buf)?;
                buf.len()
            };
            if n == 0 {
                break;
            }
            src.consume(n);
        }
        let hash = hash.finish();

        println!("hash of file is: {:x?}", hash.as_ref());
        println!("Downloaded to: {:?}", zip_file);


        // verify signature + version
        let to_be_checked = get_bytes_for_signature(hash.as_ref(), release.version.as_str());
        let remote_signature = BASE64_STANDARD.decode(target_asset.signature.as_str()).unwrap();

        let public_key = signature::UnparsedPublicKey::new(&signature::ED25519, DISTRIBUTION_PUBLIC_KEY);
        public_key.verify(to_be_checked.as_bytes(), remote_signature.as_bytes()).unwrap();

        #[cfg(feature = "signatures")]
        verify_signature(&tmp_archive_path, self.verifying_keys())?;

        println!("Extracting archive... ");
        let name = "client-gui"; //self.bin_path_in_archive();
        let bin_path_in_archive = format!("{}{}", name.trim_end_matches(EXE_SUFFIX), EXE_SUFFIX);
        let new_exe = tmp_archive_dir.path().join(&bin_path_in_archive);

        extract_file_from_zip(zip_file.as_path(), &bin_path_in_archive, &new_exe);


        println!("Done");
        println!("Replacing binary file... ");
        self_replace::self_replace(new_exe)?;
        println!("Done");

        Ok(())
    }
    pub fn restart(&self) -> Result<()> {
        let current_exe = match env::current_exe() {
            Ok(exe) => exe,
            Err(e) => bail!("Failed to restart process: {:?}", e),
        };
        println!("Restarting process: {:?}", current_exe);
        exec(process::Command::new(current_exe).args(std::env::args().into_iter().skip(1)));
        Ok(())
    }
}


fn extract_file_from_zip(zip_archive: &Path, binary_in_zip: &str, dest: &Path) {
    let archive = File::open(zip_archive).unwrap();
    let mut archive = zip::ZipArchive::new(archive).unwrap();
    let mut file = archive.by_name(binary_in_zip).unwrap();

    let mut output = File::create(dest).unwrap();
    io::copy(&mut file, &mut output).unwrap();
}