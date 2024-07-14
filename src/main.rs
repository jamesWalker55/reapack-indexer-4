mod config;
mod repo;
mod templates;
mod version;

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use log::error;
use repo::{Package, Repository, Version};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::{self},
    ops::Deref,
    path::{self, Path, PathBuf},
};
use templates::{PackageTemplateParams, RepositoryTemplateParams, VersionTemplateParams};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("repository already exists: `{0}`")]
pub(crate) struct RepositoryAlreadyExists(PathBuf);

#[derive(Error, Debug)]
#[error("package `{0}` does not exist, please use `--new` to create a new package")]
pub(crate) struct PackageDoesNotExist(String);

#[derive(Error, Debug)]
#[error("package already exists: `{0}`")]
pub(crate) struct PackageAlreadyExists(PathBuf);

#[derive(Error, Debug)]
#[error("version already exists: `{0}`")]
pub(crate) struct VersionAlreadyExists(String);

#[derive(Error, Debug)]
#[error("the source folder to publish does not exist: `{0}`")]
pub(crate) struct SourceDoesNotExist(PathBuf);

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
        #[arg(default_value = "index.xml")]
        output_path: PathBuf,
    },
    /// Add a new version of a package, by copying the given folder to the repository
    Publish {
        /// Path to the repository to add the package to
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
    /// Create a new repository
    Init {
        /// Path to the folder to initialise
        repo: PathBuf,
    },
    /// Show a configuration file template
    Template {
        /// The type of configuration to show
        #[command(subcommand)]
        template: TemplateType,
    },
}

#[derive(Subcommand)]
enum TemplateType {
    Repository,
    Package,
    Version,
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    // initialise logging
    colog::init();

    let args = Args::parse();

    match &args.command {
        Commands::Export { output_path, repo } => {
            let output_path: Cow<Path> = if output_path.exists() && output_path.metadata()?.is_dir()
            {
                output_path.join("index.xml").into()
            } else {
                output_path.into()
            };

            let repo = repo::Repository::read(repo)?;
            let index = repo.generate_index()?;
            fs::write(&output_path, index)?;
            println!("Wrote repository index to: {}", output_path.display());
        }
        Commands::Publish {
            identifier,
            version: version_name,
            path: source_path,
            repo: repo_path,
            new: should_create_new_package,
        } => {
            let repo = Repository::read(repo_path)?;

            // check that the source path exists
            if !source_path.exists() {
                return Err(SourceDoesNotExist(source_path.into()).into());
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
                if let Some(version) = version_name {
                    let sanitized_version =
                        sanitize_filename::sanitize_with_options(version, opt.clone());
                    if &sanitized_version != version {
                        return Err(InvalidPackageVersion(version.clone()).into());
                    }
                }
            }

            // get or create the package
            let pkg = if *should_create_new_package {
                repo.add_package(&identifier)?
            } else {
                let packages = repo.packages()?;
                let pkg = packages
                    .iter()
                    .find(|pkg| pkg.identifier() == identifier.as_str());
                let Some(pkg) = pkg else {
                    return Err(PackageDoesNotExist(identifier.into()).into());
                };
                pkg.clone()
            };

            // check that the version doesn't exist
            let versions = pkg.versions()?;
            let version_name: String = match version_name {
                Some(version_name) => {
                    let existing_version =
                        versions.iter().find(|v| v.name() == version_name.as_str());
                    if existing_version.is_some() {
                        return Err(VersionAlreadyExists(version_name.into()).into());
                    }
                    version_name.into()
                }
                None => match versions
                    .iter()
                    .max_by(|a, b| Version::compare_version_names(&a.name(), &b.name()))
                {
                    Some(latest_version) => Version::increment_version(&latest_version.name())?,
                    None => "0.0.1".into(),
                },
            };
            let ver_path = pkg.path().join(&version_name);
            let ver_config_path = ver_path.join("version.toml");

            // create package dir
            if !ver_path.exists() {
                fs::create_dir(&ver_path)?;
            }

            // don't create package config yet, do it after source files have been copied

            // copy the source to the version folder
            {
                let metadata = source_path.metadata()?;
                if metadata.is_dir() {
                    copy_dir_all(source_path, ver_path)?;
                } else if metadata.is_file() {
                    let dst_path = ver_path.join(source_path.file_name().unwrap());
                    fs::copy(source_path, dst_path)?;
                }
            }

            // create package config
            {
                let current_time = Utc::now().to_rfc3339();
                let config_text = templates::generate_version_config(
                    &VersionTemplateParams::default().time(&current_time),
                );
                fs::write(&ver_config_path, config_text)?;
            }

            println!("Created version {}", &version_name);
            println!(
                "Please edit the version configuration file: {}",
                ver_config_path.display()
            );
        }
        Commands::Init { repo } => {
            let repo = path::absolute(repo)?;
            let repo_config_path = repo.join("repository.toml");
            if repo_config_path.exists() {
                return Err(RepositoryAlreadyExists(repo).into());
            }

            let identifier = repo.file_name().map(|x| x.to_string_lossy());

            let mut params = RepositoryTemplateParams::default();
            if let Some(identifier) = &identifier {
                params = params.identifier(identifier);
            }
            let config_text = templates::generate_repository_config(&params);
            fs::write(&repo_config_path, config_text)?;

            println!("Created repository at {}", &path::absolute(repo)?.display());
            println!(
                "Please edit the repository configuration: {}",
                &path::absolute(repo_config_path)?.to_string_lossy()
            );
        }
        Commands::Template { template } => {
            let text = match template {
                TemplateType::Repository => {
                    templates::generate_repository_config(&RepositoryTemplateParams::default())
                }
                TemplateType::Package => {
                    templates::generate_package_config(&PackageTemplateParams::default())
                }
                TemplateType::Version => {
                    templates::generate_version_config(&VersionTemplateParams::default())
                }
            };
            println!("{}", text);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS, NON_ALPHANUMERIC};

    use super::*;

    #[test]
    fn test_01() {
        let input = "fx-chunk-data/0.0.1/Copy chunk data from last-focused FX.lua";
        let expected = "fx-chunk-data/0.0.1/Copy%20chunk%20data%20from%20last-focused%20FX.lua";
        const FRAGMENT: &AsciiSet = &NON_ALPHANUMERIC
            .remove(b'/')
            .remove(b'.')
            .remove(b'-')
            .remove(b'_');
        let result = utf8_percent_encode(input, FRAGMENT).to_string();
        assert_eq!(result, expected);
    }
}
