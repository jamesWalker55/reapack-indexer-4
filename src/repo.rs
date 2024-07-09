use anyhow::Result;
use ini::Ini;
use std::{
    fs::read_to_string,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("section [{0}] must be defined in the repository config")]
pub(crate) struct ConfigSectionMissing<'a>(&'a str);

#[derive(Error, Debug)]
#[error("`{0}` must be defined in the repository config")]
pub(crate) struct ConfigKeyMissing<'a>(&'a str);

#[derive(Debug)]
pub(crate) struct Repo {
    author: String,
    link_pattern: String,
}

impl Repo {
    pub(crate) fn read(dir: &Path) -> Result<Self> {
        let config_path = dir.join("repository.ini");
        let ini = Ini::load_from_file(config_path)?;

        let section = ini
            .section(Some("repository"))
            .ok_or(ConfigSectionMissing("repository"))?;

        let author = section.get("author").ok_or(ConfigKeyMissing("author"))?;
        let link_pattern = section
            .get("link_pattern")
            .ok_or(ConfigKeyMissing("link_pattern"))?;

        Ok(Self {
            author: author.into(),
            link_pattern: link_pattern.into(),
        })
    }
}

#[derive(Debug)]
pub(crate) struct Package {
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
        let desc = Self::read_description(dir)?;

        Ok(Self {
            r#type: r#type.into(),
            name: name.into(),
            desc,
        })
    }

    fn read_description(dir: &Path) -> Result<Option<String>> {
        let rtf_path = dir.join("README.rtf");
        if rtf_path.exists() {
            return Ok(Some(read_to_string(rtf_path)?));
        }

        let md_path = dir.join("README.md");
        if md_path.exists() {
            todo!("implement pandoc handling")
        }

        Ok(None)
    }
}
