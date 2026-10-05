// This is free and unencumbered software released into the public domain.

use asimov_sdk::directory::{StateDirectory as _, fs::StateDirectory};

fn main() -> std::io::Result<()> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(());
    };
    let directory = StateDirectory::open(path)?;
    println!("Modules available: {}", directory.has_modules());
    Ok(())
}
