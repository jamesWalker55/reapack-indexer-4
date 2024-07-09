use anyhow::Result;
use ini::Ini;
use std::path::{Path, PathBuf};
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
