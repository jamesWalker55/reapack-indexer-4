use anyhow::Result;
use ini::Ini;
use std::{
    fs::{self, read_to_string},
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("section [{0}] must be defined in the repository config")]
pub(crate) struct ConfigSectionMissing<'a>(&'a str);

#[derive(Error, Debug)]
#[error("`{0}` must be defined in the repository config")]
pub(crate) struct ConfigKeyMissing<'a>(&'a str);

#[derive(Error, Debug)]
#[error("pandoc is required for converting Markdown files to RTF, please specify the path to the pandoc executable with --pandoc")]
pub(crate) struct PandocNotInstalled;

#[derive(Error, Debug)]
#[error("pandoc returned unexpected output")]
pub(crate) struct PandocOutputError;

#[derive(Debug)]
pub(crate) struct Repo {
    /// Unique identifier for this repo.
    /// Will be used as the folder name to store the repo.
    identifier: String,
    author: String,
    link_pattern: String,
    packages: Vec<Package>,
}

impl Repo {
    pub(crate) fn read(dir: &Path) -> Result<Self> {
        // convert to absolute path to ensure we can get the folder names etc
        let dir = std::path::absolute(dir).unwrap_or(dir.to_path_buf());

        let config_path = dir.join("repository.ini");
        let ini = Ini::load_from_file(config_path)?;

        let section = ini
            .section(Some("repository"))
            .ok_or(ConfigSectionMissing("repository"))?;

        let identifier = section
            .get("identifier")
            .map(|x| x.into())
            // default to directory name
            .or(dir.file_name().map(|x| x.to_string_lossy()))
            // if directory name is somehow missing, complain about config
            .ok_or(ConfigKeyMissing("identifier"))?;
        let author = section.get("author").ok_or(ConfigKeyMissing("author"))?;
        let link_pattern = section
            .get("link_pattern")
            .ok_or(ConfigKeyMissing("link_pattern"))?;

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
    /// Unique identifier for this package.
    /// Will be used as the folder name to store the package.
    identifier: String,
    /// Descriptive display name for this package, as shown in Reapack.
    name: String,
    r#type: String,
    desc: Option<String>,
}

impl Package {
    pub(crate) fn read(dir: &Path) -> Result<Self> {
        let config_path = dir.join("package.ini");
        let ini = Ini::load_from_file(config_path)?;

        let section = ini
            .section(Some("package"))
            .ok_or(ConfigSectionMissing("package"))?;

        let r#type = section.get("type").ok_or(ConfigKeyMissing("type"))?;
        let name = section
            .get("name")
            .map(|x| x.into())
            // default to directory name
            .or(dir.file_name().map(|x| x.to_string_lossy()))
            // if directory name is somehow missing, complain about config
            .ok_or(ConfigKeyMissing("name"))?;
        let identifier = section
            .get("identifier")
            .map(|x| x.into())
            // default to directory name
            .or(dir.file_name().map(|x| x.to_string_lossy()))
            // if directory name is somehow missing, complain about config
            .ok_or(ConfigKeyMissing("identifier"))?;
        let desc = Self::read_description(dir)?;

        Ok(Self {
            identifier: identifier.into(),
            r#type: r#type.into(),
            name: name.into(),
            desc,
        })
    }

    fn read_description(dir: &Path) -> Result<Option<String>> {
        let rtf_path = dir.join("README.rtf");
        if rtf_path.exists() {
            return Ok(Some(fs::read_to_string(rtf_path)?));
        }

        let md_path = dir.join("README.md");
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
}
