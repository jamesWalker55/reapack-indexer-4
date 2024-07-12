mod repo;
mod templates;
mod version;

use std::{
    collections::HashSet,
    fs::{self, File},
    io::BufWriter,
    path::PathBuf,
};

use anyhow::Result;
use clap::{Parser, Subcommand};
use repo::Package;
use templates::PackageTemplateParams;
use thiserror::Error;
use version::increment_version;

#[derive(Error, Debug)]
#[error("repository does not exist: `{0}`")]
pub(crate) struct RepositoryDoesNotExist(PathBuf);

#[derive(Error, Debug)]
#[error("package does not exist, please use `--new` to create a new package: `{0}`")]
pub(crate) struct PackageDoesNotExist(PathBuf);

#[derive(Error, Debug)]
#[error("package already exists: `{0}`")]
pub(crate) struct PackageAlreadyExists(PathBuf);

#[derive(Error, Debug)]
#[error("version already exists: `{0}`")]
pub(crate) struct VersionAlreadyExists(String);

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
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a ReaPack XML index file
    Export {
        /// Path to the folder to be processed
        #[arg(short, long)]
        repo: PathBuf,
        /// Path to write the generated Reapack index XML file
        output_path: PathBuf,
    },
    /// Add a new version of a package, by copying the given folder to the repository
    Publish {
        /// Path to the folder to be processed
        #[arg(short, long)]
        repo: PathBuf,
        /// Whether to create a new package or not
        #[arg(short, long, default_value_t = false)]
        new: bool,
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

    match &args.command {
        Commands::Export { output_path, repo } => {
            let repo = repo::Repo::read(&repo)?;
            let f = File::create(output_path)?;
            let mut f = BufWriter::new(f);
            repo.generate_index(&mut f).unwrap();
        }
        Commands::Publish {
            identifier,
            version,
            path,
            repo,
            new,
        } => {
            // check that repository exists
            if !repo.join("repository.ini").exists() {
                return Err(RepositoryDoesNotExist(repo.into()).into());
            }

            // check that the identifier and version are sane
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

            let pkg_path = repo.join(identifier);
            let pkg_config_path = pkg_path.join("package.ini");
            if *new {
                if pkg_config_path.exists() {
                    return Err(PackageAlreadyExists(pkg_path.into()).into());
                }

                // create package dir
                if !pkg_path.exists() {
                    fs::create_dir(&pkg_path)?;
                }

                // create package config
                let config_text = templates::generate_package_config(
                    &PackageTemplateParams::default()
                        .name(&identifier)
                        .identifier(&identifier),
                );
                fs::write(&pkg_config_path, config_text)?;

                println!("Created package {}", &identifier);
                println!(
                    "Please edit the package configuration: {}",
                    &pkg_config_path.to_string_lossy()
                );
            } else {
                // use existing repo
                if !pkg_config_path.exists() {
                    return Err(PackageDoesNotExist(pkg_path.into()).into());
                }
            }

            let versions: HashSet<_> = Package::get_version_paths(&pkg_path)?
                .into_iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect();
            let new_version: String = match version {
                Some(version) => {
                    if versions.contains(version.into()) {
                        return Err(VersionAlreadyExists(version.into()).into());
                    }
                    version.into()
                }
                None => match versions.iter().max() {
                    Some(latest_version) => increment_version(&latest_version)?,
                    None => "0.0.1".into(),
                },
            };
            // let ver_path = pkg_path.join(version)

            // if !pkg_path.exists() {
            //     fs::create_dir(&pkg_path)?;
            //     println!("Created package {}", &identifier);
            // }

            // if !pkg_config_path.exists() {
            //     let config_text = templates::generate_package_config(
            //         &PackageTemplateParams::default()
            //             .name(&identifier)
            //             .identifier(&identifier),
            //     );
            //     fs::write(&pkg_config_path, config_text)?;
            //     println!(
            //         "Created initial package configuration: {}",
            //         &pkg_config_path.to_string_lossy()
            //     );
            // }

            todo!()
        }
    }

    Ok(())
}
