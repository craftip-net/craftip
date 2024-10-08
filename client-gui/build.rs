use std::fs::File;
use std::io::Write;
use {
    std::{env, io},
    winres::WindowsResource,
};

fn main() -> io::Result<()> {
    if let Ok(file) = env::var("COMPILE_VERSION_FILE") {
        let version = env::var("CARGO_PKG_VERSION").unwrap();
        File::create(file).unwrap().write_all(version.as_bytes()).unwrap();
    }
    
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        WindowsResource::new()
            // This path can be absolute, or relative to your crate root.
            .set_icon("../build/resources/logo-win.ico")
            .compile()?;
    }
    Ok(())
}
