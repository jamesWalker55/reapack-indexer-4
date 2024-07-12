mod repo;
mod templates;

use std::{fs::File, io::BufWriter, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("the package name is not filename-safe, please choose a different package name: `{0}`")]
pub(crate) struct InvalidPackageName(String);

#[derive(Error, Debug)]
#[error(
    "the package version is not filename-safe, please choose a different package version: `{0}`"
)]
pub(crate) struct InvalidPackageVersion(String);

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
    /// Add a new version of a package, by copying the given folder to the repository
    Publish {
        /// Name of the package
        #[arg(short, long)]
        identifier: String,
        /// Path to the folder to be copied
        path: PathBuf,
        /// Version of the package
        version: Option<String>,
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
        Commands::Publish {
            identifier,
            version,
            path,
        } => {
            // check that the identifier and version is sane
            {
                let opt = sanitize_filename::Options {
                    truncate: true,  // true by default, truncates to 255 bytes
                    windows: true, // default value depends on the OS, removes reserved names like `con` from start of strings on Windows
                    replacement: "", // str to replace sanitized chars/strings
                };
                let sanitized_identifier =
                    sanitize_filename::sanitize_with_options(identifier, opt.clone());
                if &sanitized_identifier != identifier {
                    return Err(InvalidPackageName(identifier.clone()).into());
                }
                if let Some(version) = version {
                    let sanitized_version =
                        sanitize_filename::sanitize_with_options(version, opt.clone());
                    if &sanitized_version != version {
                        return Err(InvalidPackageVersion(version.clone()).into());
                    }
                }
            }

            todo!()
        }
    }

    Ok(())
}
