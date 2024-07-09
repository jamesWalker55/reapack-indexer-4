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
    Ok(())
}
