use semver::Version;
use shared::config::UPDATE_URL;
use std::env::consts::EXE_SUFFIX;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::{env, io, process};
use base64::prelude::*;
use image::EncodableLayout;
use ring::digest::{Context, SHA512};
use ring::signature;
use crate::updater_proto::{decompress, DISTRIBUTION_PUBLIC_KEY, get_bytes_for_signature, LatestRelease, Target, UpdaterError};

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


#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub changelog: String,
    pub size: usize,
}

#[derive(Debug)]
pub struct Updater {
    target: Target,
    version: String,
    changelog: String,
}


impl Updater {
    pub fn new() -> Result<Option<Self>, UpdaterError> {
        //set_ssl_vars!();
        let api_url = UPDATE_URL.to_string();

        let resp = ureq::get(&api_url).call()?;

        println!("hello from the updater");
        let release = resp.into_body().read_json::<LatestRelease>().map_err(UpdaterError::ParsingError)?;
        let version = Version::parse(&release.version).map_err(|_|UpdaterError::CouldNotParseVersion)?;

        // if local version up to date
        if Version::parse(CURRENT_VERSION).unwrap() >= version {
            return Ok(None)
        }

        println!("New version available: v{}", release.version);
        let target = release.targets
            .into_iter()
            .find(|t| t.target == current_platform::CURRENT_PLATFORM)
            .ok_or(UpdaterError::TargetNotFound)?;

        Ok(Some(Self {
            changelog: release.changelog,
            target,
            version: release.version,
        }))
    }

    pub fn get_update_info(&self) -> UpdateInfo {
        UpdateInfo {
            version: self.version.clone(),
            changelog: self.changelog.clone(),
            size: self.target.size as usize,
        }
    }

    pub fn update(&self) -> Result<(), UpdaterError> {
        println!(
            "Checking target-arch... {}",
            current_platform::CURRENT_PLATFORM
        );
        println!("Checking current version... v{}", CURRENT_VERSION);

        println!("Checking latest released version... ");

        println!("v{:?}", self.version);


        let tmp_archive_dir = tempfile::TempDir::new().map_err(UpdaterError::IoError)?;
        let archive = tmp_archive_dir.path().join(&self.target.name);

        println!("Downloading...");


        let resp = ureq::get(&self.target.url).call()?;
        let resp = resp.into_body().into_reader();
        let mut out = File::create(&archive).expect("failed to create file");

        let mut hash = Context::new(&SHA512);
        let mut src = BufReader::new(resp);
        loop {
            let n = {
                let buf = src.fill_buf().map_err(UpdaterError::IoError)?;
                hash.update(buf);
                out.write_all(buf).map_err(UpdaterError::IoError)?;
                buf.len()
            };
            if n == 0 {
                break;
            }
            src.consume(n);
        }
        let hash = hash.finish();

        println!("hash of file is: {:x?}", hash.as_ref());
        println!("Downloaded to: {:?}", archive);


        // verify signature + version
        let to_be_checked = get_bytes_for_signature(hash.as_ref(), self.version.as_str());
        let remote_signature = BASE64_STANDARD.decode(self.target.signature.as_str())?;

        let public_key = signature::UnparsedPublicKey::new(&signature::ED25519, DISTRIBUTION_PUBLIC_KEY);
        public_key.verify(to_be_checked.as_bytes(), remote_signature.as_bytes()).map_err(|_|UpdaterError::SignatureMatchFailed)?;

        println!("Extracting archive... ");
        let name = "client-gui";
        let bin_path_in_archive = format!("{}{}", name.trim_end_matches(EXE_SUFFIX), EXE_SUFFIX);
        let new_exe = tmp_archive_dir.path().join(&bin_path_in_archive);

        decompress(archive.as_path(), &new_exe)?;

        println!("Done");
        println!("Replacing binary file... ");
        self_replace::self_replace(new_exe).map_err(UpdaterError::ReplaceFailed)?;
        println!("Done");

        Ok(())
    }
    pub fn restart(&self) {
        let current_exe = match env::current_exe() {
            Ok(exe) => exe,
            Err(e) => panic!("Failed to restart process: {:?}", e),
        };
        println!("Restarting process: {:?}", current_exe);
        exec(process::Command::new(current_exe).args(std::env::args().into_iter().skip(1)));
    }
}
