mod repo;

use std::{fs::File, io::BufWriter, path::PathBuf};

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

    let repo = repo::Repo::read(&args.repo_path)?;

    {
        let f = File::create(args.output_path)?;
        let mut f = BufWriter::new(f);
        repo.generate_index(&mut f).unwrap();
    }

    Ok(())
}
