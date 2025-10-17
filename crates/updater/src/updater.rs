use crate::config::UPDATE_URL;
use crate::reporter::report_update_error;
use crate::updater_proto::{decompress, verify_signature, LatestRelease, Target, UpdaterError};
use ring::digest::{Context, SHA512};
use semver::Version;
use std::env::consts::EXE_SUFFIX;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::{env, io, process};
use ureq::Agent;

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
    agent: Agent,
}

impl Updater {
    pub fn new(current_version: &str) -> Result<Option<Self>, UpdaterError> {
        //set_ssl_vars!();
        let user_agent = format!(
            "CraftIP Updater v{current_version} {}",
            current_platform::CURRENT_PLATFORM
        );
        let agent = ureq::builder().user_agent(user_agent.as_str()).build();

        let resp = agent.get(UPDATE_URL).call()?;

        println!("Updater started with v{current_version}...");
        let release = resp
            .into_json::<LatestRelease>()
            .map_err(UpdaterError::ParsingError)?;

        let version =
            Version::parse(&release.version).map_err(|_| UpdaterError::CouldNotParseVersion)?;

        // if local version up to date
        if Version::parse(current_version).unwrap() >= version {
            return Ok(None);
        }

        println!("New version available: v{}", release.version);
        let target = release
            .targets
            .into_iter()
            .find(|t| t.target == current_platform::CURRENT_PLATFORM)
            .ok_or(UpdaterError::TargetNotFound)?;

        Ok(Some(Self {
            changelog: release.changelog,
            target,
            version: release.version,
            agent,
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
        let update = || -> Result<(), UpdaterError> {
            println!(
                "Checking target-arch... {}",
                current_platform::CURRENT_PLATFORM
            );

            println!("Checking latest released version... ");

            println!("v{:?}", self.version);

            let tmp_archive_dir = tempfile::TempDir::new().map_err(UpdaterError::IoError)?;
            let archive_name = "client-gui.xz";
            let archive = tmp_archive_dir.path().join(archive_name);

            println!("Downloading...");

            let resp = self.agent.get(&self.target.url).call()?;
            let resp = resp.into_reader();
            let mut out = File::create(&archive)?;

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

            println!("Downloaded to: {:?}", archive);

            verify_signature(
                hash.as_ref(),
                self.version.as_ref(),
                self.target.signature.as_str(),
            )?;

            println!("Extracting archive... ");
            let exe_name = "client-gui";
            let exe_name = format!("{}{}", exe_name.trim_end_matches(EXE_SUFFIX), EXE_SUFFIX);
            let new_exe = tmp_archive_dir.path().join(&exe_name);

            decompress(archive.as_path(), &new_exe)?;

            println!("Done");
            println!("Replacing binary file... ");
            self_replace::self_replace(new_exe).map_err(UpdaterError::ReplaceFailed)?;
            println!("Done");

            Ok(())
        };

        let res = update();
        if let Err(err) = &res {
            report_update_error(self.agent.clone(), err, &self.version);
        }

        res
    }
    pub fn restart(&self) -> Result<(), UpdaterError> {
        let restart = || -> Result<(), UpdaterError> {
            let current_exe = match env::current_exe() {
                Ok(exe) => exe,
                Err(e) => return Err(UpdaterError::RestartFailed(e)),
            };
            println!("Restarting process: {:?}", current_exe);
            #[allow(clippy::useless_conversion)]
            let e =
                exec(process::Command::new(current_exe).args(std::env::args().into_iter().skip(1)));
            // should never be called
            Err(UpdaterError::RestartFailed(e))
        };

        let res = restart();
        if let Err(err) = &res {
            report_update_error(self.agent.clone(), err, &self.version);
        }
        res
    }
}
