mod repo;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

/// Generate a Reapack index
#[derive(Parser, Debug)]
struct Args {
    /// Path to the folder to be processed
    repo_path: PathBuf,
    /// Path to write the generated Reapack index XML file
    #[arg(short, long, default_value = "index.xml")]
    output_path: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    dbg!(&args);
    let repo = repo::Repo::read(&args.repo_path)?;
    dbg!(&repo);

    let mut buf: Vec<u8> = Vec::new();
    repo.generate_index(&mut buf).unwrap();
    let output = std::str::from_utf8(buf.as_slice()).unwrap().to_string();
    println!("{}", output);

    Ok(())
}
