mod repo;

use std::{fs::File, io::BufWriter, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Generate a Reapack index
#[derive(Parser)]
struct Args {
    /// Path to the folder to be processed
    #[arg(short, long)]
    repo: PathBuf,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a ReaPack XML index file
    Export {
        /// Path to write the generated Reapack index XML file
        output_path: PathBuf,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    let repo = repo::Repo::read(&args.repo)?;

    match &args.command {
        Commands::Export { output_path } => {
            let f = File::create(output_path)?;
            let mut f = BufWriter::new(f);
            repo.generate_index(&mut f).unwrap();
        }
    }

    Ok(())
}
