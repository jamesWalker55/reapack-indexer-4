mod config;
mod repo;
mod templates;
mod version;

use std::{
    borrow::Cow,
    collections::HashSet,
    fs::{self, File},
    io::BufWriter,
    path::{self, Path, PathBuf},
};

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use repo::Package;
use templates::{PackageTemplateParams, RepositoryTemplateParams, VersionTemplateParams};
use thiserror::Error;
use version::{find_latest_version, increment_version};

#[derive(Error, Debug)]
#[error("repository does not exist: `{0}`")]
pub(crate) struct RepositoryDoesNotExist(PathBuf);

#[derive(Error, Debug)]
#[error("repository already exists: `{0}`")]
pub(crate) struct RepositoryAlreadyExists(PathBuf);

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
    let args = Args::parse();

    match &args.command {
        Commands::Export { output_path, repo } => {
            let output_path: Cow<Path> = if output_path.exists() && output_path.metadata()?.is_dir()
            {
                output_path.join("index.xml").into()
            } else {
                output_path.into()
            };

            let repo = repo::Repo::read(repo)?;
            let index = repo.generate_index()?;
            fs::write(&output_path, &index)?;
            println!("Wrote repository index to: {}", output_path.display());
        }
        Commands::Publish {
            identifier,
            version,
            path,
            repo,
            new,
        } => {
            // check that repository exists
            if !repo.join("repository.toml").exists() {
                return Err(RepositoryDoesNotExist(repo.into()).into());
            }

            // check that the source path exists
            if !path.exists() {
                return Err(SourceDoesNotExist(path.into()).into());
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
            let pkg_config_path = pkg_path.join("package.toml");
            if *new {
                if pkg_config_path.exists() {
                    return Err(PackageAlreadyExists(pkg_path).into());
                }

                // create package dir
                if !pkg_path.exists() {
                    fs::create_dir(&pkg_path)?;
                }

                // create package config
                let config_text = templates::generate_package_config(
                    &PackageTemplateParams::default()
                        .name(identifier)
                        .identifier(identifier),
                );
                fs::write(&pkg_config_path, config_text)?;

                println!("Created package {}", &identifier);
                println!(
                    "Please edit the package configuration: {}",
                    &path::absolute(pkg_config_path)?.to_string_lossy()
                );
            } else {
                // use existing repo
                if !pkg_config_path.exists() {
                    return Err(PackageDoesNotExist(pkg_path).into());
                }
            }

            let versions: HashSet<_> = Package::get_version_paths(&pkg_path)?
                .into_iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect();
            let new_version: String = match version {
                Some(version) => {
                    if versions.contains(version) {
                        return Err(VersionAlreadyExists(version.into()).into());
                    }
                    version.into()
                }
                None => match find_latest_version(versions.iter().map(|x| x.as_ref())) {
                    Some(latest_version) => increment_version(latest_version)?,
                    None => "0.0.1".into(),
                },
            };
            let ver_path = pkg_path.join(&new_version);
            let ver_config_path = ver_path.join("version.toml");

            // create package dir
            if !ver_path.exists() {
                fs::create_dir(&ver_path)?;
            }

            // don't create package config yet, do it after source files have been copied

            // copy the source to the version folder
            {
                let metadata = path.metadata()?;
                if metadata.is_dir() {
                    copy_dir_all(path, ver_path)?;
                } else if metadata.is_file() {
                    let dst_path = ver_path.join(path.file_name().unwrap());
                    fs::copy(path, dst_path)?;
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

            println!("Created version {}", &new_version);
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
