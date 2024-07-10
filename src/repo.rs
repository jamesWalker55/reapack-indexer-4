use anyhow::Result;
use chrono::DateTime;
use ini::Ini;
use std::{
    fs::{self, read_to_string},
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("section [{0}] must be defined in the config at: {1}\ndetails: {2}")]
pub(crate) struct ConfigSectionMissing<'a>(&'a str, PathBuf, &'a str);

#[derive(Error, Debug)]
#[error("`{0}` must be defined in the config at: {1}\ndetails: {2}")]
pub(crate) struct ConfigKeyMissing<'a>(&'a str, PathBuf, &'a str);

#[derive(Error, Debug)]
#[error("pandoc is required for converting Markdown files to RTF, please specify the path to the pandoc executable with --pandoc")]
pub(crate) struct PandocNotInstalled;

#[derive(Error, Debug)]
#[error("pandoc returned unexpected output")]
pub(crate) struct PandocOutputError;

/// Try to read an RTF file at the given path.
/// If no RTF file is found, read and convert a Markdown file to RTF.
/// If no Markdown file is found, return None.
fn read_rtf_or_md_file(path: &Path) -> Result<Option<String>> {
    let rtf_path = path.with_extension("rtf");
    if rtf_path.exists() {
        return Ok(Some(fs::read_to_string(rtf_path)?));
    }

    let md_path = path.with_extension("md");
    if md_path.exists() {
        let mut pandoc = pandoc::new();
        // TODO: Allow overriding pandoc path
        // pandoc.add_pandoc_path_hint(custom_path);
        pandoc.add_input(&md_path);
        pandoc.add_option(pandoc::PandocOption::Standalone);
        pandoc.set_output(pandoc::OutputKind::Pipe);
        pandoc.set_output_format(pandoc::OutputFormat::Rtf, vec![]);
        // pandoc::PandocError::PandocNotFound
        let output = pandoc.execute().map_err(|e| match e {
            pandoc::PandocError::PandocNotFound => anyhow::Error::from(PandocNotInstalled),
            e => e.into(),
        })?;
        let pandoc::PandocOutput::ToBuffer(output) = output else {
            return Err(PandocOutputError.into());
        };
        return Ok(Some(output));
    }

    Ok(None)
}

#[derive(Debug)]
pub(crate) struct Repo {
    /// Unique identifier for this repo.
    /// Will be used as the folder name to store the repo.
    identifier: String,
    author: String,
    link_pattern: String,
    packages: Vec<Package>,
    desc: Option<String>,
}

impl Repo {
    pub(crate) fn read(dir: &Path) -> Result<Self> {
        // convert to absolute path to ensure we can get the folder names etc
        let dir = std::path::absolute(dir).unwrap_or(dir.to_path_buf());

        let config_path = dir.join("repository.ini");
        let ini = Ini::load_from_file(&config_path)?;

        let section = ini.section(Some("repository")).ok_or(ConfigSectionMissing(
            "repository",
            config_path.clone(),
            "https://github.com/cfillion/reapack/wiki/Index-Format#index-element",
        ))?;

        let identifier = section
            .get("identifier")
            .map(|x| x.into())
            // default to directory name
            .or(dir.file_name().map(|x| x.to_string_lossy()))
            // if directory name is somehow missing, complain about config
            .ok_or(ConfigKeyMissing("identifier", config_path.clone(), "The unique identifier for this repository. Should be unique to avoid conflicts with other folders with the same name."))?;
        let author = section.get("author").ok_or(ConfigKeyMissing(
            "author",
            config_path.clone(),
            "The author of packages within this repository",
        ))?;
        let link_pattern = section
            .get("link_pattern")
            .ok_or(ConfigKeyMissing("link_pattern", config_path.clone(), "A string template that is used to generate the URLs of package source files. E.g.: https://raw.githubusercontent.com/YOUR_USERNAME/YOUR_REPOSITORY/{latest_commit}/{relpath}"))?;
        let desc = read_rtf_or_md_file(&dir.join("README.rtf"))?;

        let packages: Result<Vec<Package>> = Self::get_package_paths(&dir)?
            .iter()
            .map(|p| Package::read(&p))
            .collect();
        let packages = packages?;

        Ok(Self {
            identifier: identifier.into(),
            author: author.into(),
            link_pattern: link_pattern.into(),
            packages,
            desc,
        })
    }

    fn get_package_paths(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut paths = vec![];
        for path in fs::read_dir(dir)? {
            let path = path?;
            let is_dir = path.metadata()?.is_dir();
            if is_dir {
                paths.push(path.path());
            }
        }
        Ok(paths)
    }
}

#[derive(Debug)]
pub(crate) struct Package {
    path: PathBuf,
    /// Unique identifier for this package.
    /// Will be used as the folder name to store the package.
    identifier: String,
    /// Descriptive display name for this package, as shown in Reapack.
    name: String,
    r#type: String,
    desc: Option<String>,
    versions: Vec<PackageVersion>,
}

impl Package {
    pub(crate) fn read(dir: &Path) -> Result<Self> {
        let config_path = dir.join("package.ini");
        let ini = Ini::load_from_file(&config_path)?;

        let section = ini.section(Some("package")).ok_or(ConfigSectionMissing(
            "package",
            config_path.clone(),
            "https://github.com/cfillion/reapack/wiki/Index-Format#reapack-element",
        ))?;

        let r#type = section.get("type").ok_or(ConfigKeyMissing(
            "type",
            config_path.clone(),
            "Possible values are script, effect, extension, data, theme, langpack, webinterface, projectpl, tracktpl, midinotenames and autoitem",
        ))?;
        let name = section
            .get("name")
            .map(|x| x.into())
            // default to directory name
            .or(dir.file_name().map(|x| x.to_string_lossy()))
            // if directory name is somehow missing, complain about config
            .ok_or(ConfigKeyMissing(
                "name",
                config_path.clone(),
                "This is the display name of the package, as seen in Reapack's package list browser",
            ))?;
        let identifier = section
            .get("identifier")
            .map(|x| x.into())
            // default to directory name
            .or(dir.file_name().map(|x| x.to_string_lossy()))
            // if directory name is somehow missing, complain about config
            .ok_or(ConfigKeyMissing("identifier", config_path.clone(), "This is the name of the folder where the package will be stored, defaults to the current package's folder name"))?;
        let desc = read_rtf_or_md_file(&dir.join("README.rtf"))?;

        let versions: Result<Vec<PackageVersion>> = Self::get_version_paths(&dir)?
            .iter()
            .map(|p| PackageVersion::read(&p))
            .collect();
        let versions = versions?;

        Ok(Self {
            path: dir.into(),
            identifier: identifier.into(),
            r#type: r#type.into(),
            name: name.into(),
            desc,
            versions,
        })
    }

    fn get_version_paths(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut paths = vec![];
        for path in fs::read_dir(dir)? {
            let path = path?;
            let is_dir = path.metadata()?.is_dir();
            if is_dir {
                paths.push(path.path());
            }
        }
        Ok(paths)
    }
}

#[derive(Debug)]
pub(crate) struct PackageVersion {
    path: PathBuf,
    /// The version name, e.g. '0.0.1'
    name: String,
    time: DateTime<chrono::Utc>,
}

impl PackageVersion {
    pub(crate) fn read(dir: &Path) -> Result<Self> {
        let config_path = dir.join("version.ini");
        let ini = Ini::load_from_file(&config_path)?;

        let section = ini.section(Some("version")).ok_or(ConfigSectionMissing(
            "version",
            config_path.clone(),
            "https://github.com/cfillion/reapack/wiki/Index-Format#version-element",
        ))?;

        let name = section
            .get("version")
            .map(|x| x.into())
            // default to directory name
            .or(dir.file_name().map(|x| x.to_string_lossy()))
            // if directory name is somehow missing, complain about config
            .ok_or(ConfigKeyMissing(
                "version",
                config_path.clone(),
                "This should be the version name, e.g. '0.0.1'. Defaults to the current version's folder name",
            ))?;

        let temp = inquire::Confirm::new("Version publication time is not found. Would you like to use the current time as the publication time?")
            .with_default(true)
            .with_help_message("The publication time will be set in version.ini under [version] and key `version`.")
            .prompt()?;

        dbg!(temp);

        todo!();

        // let time: DateTime<chrono::Utc> = {
        //     let user_time = section
        //         .get("time")
        //         .map(|x| x.into())
        //     // chrono::Utc::now()
        //     section
        //         .get("time")
        //         .map(|x| x.into())
        //         // default to directory name
        //         .or(dir.file_name().map(|x| x.to_string_lossy()))
        //         // if directory name is somehow missing, complain about config
        //         .ok_or(ConfigKeyMissing("time"))?
        // };

        // DateTime::parse_from_rfc3339()

        // Ok(Self {
        //     path: dir,
        //     name: name.into(),
        //     time: todo!(),
        // })
    }
}
