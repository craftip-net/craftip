use anyhow::{bail, format_err, Result};
use reqwest::header;
use self_update::{cargo_crate_version, get_target, self_replace, version, Download, Extract};
use serde::{Deserialize, Serialize};
use shared::config::UPDATE_URL;
use std::env::consts::EXE_SUFFIX;
use std::{env, fs, io, process};

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

#[derive(Default)]
pub struct Updater {
    release: Option<LatestRelease>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct LatestRelease {
    version: String,
    changelog: String,
    targets: Vec<Target>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub name: String,
    pub url: String,
    pub target: String,
}
impl Updater {
    pub fn check_for_update(&mut self) -> Result<bool> {
        let current_version = cargo_crate_version!();
        //set_ssl_vars!();
        let api_url = UPDATE_URL.to_string();

        let resp = reqwest::blocking::Client::new().get(&api_url).send()?;
        if !resp.status().is_success() {
            bail!(
                "api request failed with status: {:?} - for: {:?}",
                resp.status(),
                api_url
            )
        }
        let release = resp.json::<LatestRelease>()?;

        println!(
            "New release found! v{} --> v{}",
            current_version, release.version
        );

        if !version::bump_is_compatible(&current_version, &release.version)? {
            println!("New release is a bit tooooo new compatible");
            //bail!("New release is too new and therefore not compatible");
        };
        let new_version = version::bump_is_greater(&current_version, &release.version)?;
        self.release = Some(release);

        Ok(new_version)
    }
    pub fn update(&mut self) -> Result<()> {
        let target = get_target();

        println!("Checking target-arch... {}", target);
        println!("Checking current version... v{}", cargo_crate_version!());

        println!("Checking latest released version... ");

        let release = self.release.as_ref().unwrap();
        println!("v{:?}", release);

        let target_asset = release
            .targets
            .iter()
            .find(|t| t.target == target)
            .ok_or_else(|| format_err!("No release found for target: {}", target))?;

        //let prompt_confirmation = !self.no_confirm();
        println!("\n{} release status:", release.version);
        println!("  * New exe release: {:?}", target_asset.name);
        println!("  * New exe download url: {:?}", target_asset.url);
        println!("\nThe new release will be downloaded/extracted and the existing binary will be replaced.");

        let tmp_archive_dir = tempfile::TempDir::new()?;
        let tmp_archive_path = tmp_archive_dir.path().join(&target_asset.name);
        let mut tmp_archive = fs::File::create(&tmp_archive_path)?;

        println!("Downloading...");
        let mut download = Download::from_url(&target_asset.url);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::ACCEPT, "application/octet-stream".parse().unwrap());
        download.set_headers(headers);
        download.show_progress(true);

        download.download_to(&mut tmp_archive)?;

        #[cfg(feature = "signatures")]
        verify_signature(&tmp_archive_path, self.verifying_keys())?;

        println!("Extracting archive... ");
        let name = "client-gui"; //self.bin_path_in_archive();
        let bin_path_in_archive = format!("{}{}", name.trim_end_matches(EXE_SUFFIX), EXE_SUFFIX);
        Extract::from_source(&tmp_archive_path)
            .extract_file(tmp_archive_dir.path(), &bin_path_in_archive)?;
        let new_exe = tmp_archive_dir.path().join(&bin_path_in_archive);

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
