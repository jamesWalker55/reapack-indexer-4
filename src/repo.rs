use anyhow::Result;
use chrono::DateTime;
use ini::Ini;
use relative_path::RelativePathBuf;
use std::{
    collections::HashMap,
    fs::{self, read_to_string},
    path::{Path, PathBuf},
};
use thiserror::Error;
use xml_builder::{XMLBuilder, XMLElement, XMLVersion};

#[derive(Error, Debug)]
#[error("section [{0}] must be defined in the config at: {1}\ndetails: {2}")]
pub(crate) struct ConfigSectionMissing<'a>(&'a str, PathBuf, &'a str);

#[derive(Error, Debug)]
#[error("`{0}` must be defined in the config at: {1}\ndetails: {2}")]
pub(crate) struct ConfigKeyMissing<'a>(&'a str, PathBuf, &'a str);

#[derive(Error, Debug)]
#[error("category for package `{0}` cannot be an absolute path: {1}")]
pub(crate) struct PackageCategoryCannotBeAbsolutePath(String, PathBuf);

#[derive(Error, Debug)]
#[error("category for package `{0}` cannot contain '../': {1}")]
pub(crate) struct PackageCategoryCannotContainParentDir(String, PathBuf);

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

fn cdata(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 12);
    result.push_str("<![CDATA[");

    let mut is_first_part = true;
    for part in text.split("]]>") {
        if is_first_part {
            result.push_str(part);
            is_first_part = false;
        } else {
            result.push_str("]]]]><![CDATA[>");
            result.push_str(part);
        }
    }

    result.push_str("]]>");
    result
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
            .ok_or(ConfigKeyMissing("link_pattern", config_path.clone(), "A string template that is used to generate the URLs of package source files. E.g.: https://raw.githubusercontent.com/YOUR_USERNAME/YOUR_REPOSITORY/{git_commit}/{relpath}"))?;
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

    pub(crate) fn generate_index<W: std::io::Write>(
        &self,
        writer: &mut W,
    ) -> xml_builder::Result<()> {
        let mut xml = XMLBuilder::new()
            .version(XMLVersion::XML1_1)
            .encoding("UTF-8".into())
            .build();

        let root_element = self.element();
        xml.set_root_element(root_element);

        xml.generate(writer)
    }

    fn element(&self) -> XMLElement {
        let mut index = XMLElement::new("index");
        index.add_attribute("version", "1");
        index.add_attribute("name", &self.identifier);

        // add description
        if let Some(desc) = &self.desc {
            let mut metadata = XMLElement::new("metadata");
            let mut description = XMLElement::new("description");
            description.add_text(cdata(desc)).unwrap();
            metadata.add_child(description).unwrap();
            index.add_child(metadata).unwrap();
        }

        // group packages into categories
        let pkg_map = {
            let mut pkg_map = HashMap::new();
            for pkg in self.packages.iter() {
                if !pkg_map.contains_key(&pkg.category) {
                    pkg_map.insert(&pkg.category, vec![]);
                }
                let packages = pkg_map.get_mut(&pkg.category).unwrap();
                packages.push(pkg)
            }
            pkg_map
        };

        // insert categories into index
        for (category_name, packages) in pkg_map.iter() {
            let mut category = XMLElement::new("category");
            category.add_attribute("name", &category_name.to_string());

            for pkg in packages {
                let reapack = pkg.element();
                category.add_child(reapack).unwrap();
            }

            index.add_child(category).unwrap();
        }

        index
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
    /// Category for classification, not target folder.
    /// Must be a Path so I can reverse-engineer how many '../' to add to the source path.
    category: RelativePathBuf,
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
        let category = {
            let text = section.get("category").ok_or(ConfigKeyMissing(
                "category",
                config_path.clone(),
                "Used for organization in the package list. (Unlike the official tool, this does not control the target directory)",
            ))?;
            let relpath = RelativePathBuf::from_path(text)
                .map(|p| p.normalize())
                .map_err(|_| {
                    PackageCategoryCannotBeAbsolutePath(name.to_string(), config_path.clone())
                })?;
            if relpath.starts_with("..") {
                Err(PackageCategoryCannotContainParentDir(
                    name.to_string(),
                    config_path.clone(),
                ))
            } else {
                Ok(relpath)
            }
        }?;
        let desc = read_rtf_or_md_file(&dir.join("README.rtf"))?;

        let versions: Result<Vec<PackageVersion>> = Self::get_version_paths(&dir)?
            .iter()
            .map(|p| PackageVersion::read(&p))
            .collect();
        let versions = versions?;

        Ok(Self {
            path: dir.into(),
            identifier: identifier.into(),
            category: category.into(),
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

    fn element(&self) -> XMLElement {
        todo!()
    }
}

#[derive(Debug)]
pub(crate) struct PackageVersion {
    path: PathBuf,
    /// The version name, e.g. '0.0.1'
    name: String,
    time: DateTime<chrono::Utc>,
    changelog: Option<String>,
    sources: Vec<Source>,
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

        let time: std::result::Result<DateTime<chrono::Utc>, ConfigKeyMissing> = {
            match section.get("time") {
                Some(user_time) => match DateTime::parse_from_rfc3339(user_time) {
                    Ok(user_time) => Ok(user_time.into()),
                    Err(e) => {
                        let prompt_msg = format!("Failed to parse version publication time set in: {}\nError: {:?}\nWould you like to use the current time as the publication time? (The publication time will be written to version.ini)", config_path.to_string_lossy(), e);
                        let prompt = inquire::Confirm::new(&prompt_msg)
                            .with_default(false)
                            .prompt()?;
                        if !prompt {
                            Err(ConfigKeyMissing("time", config_path.clone(), "This should be the publication time, using the RFC 3339 / ISO 8601 format. E.g. 1996-12-19T16:39:57-08:00"))
                        } else {
                            let time = chrono::Utc::now();

                            let mut ini = ini.clone();

                            ini.with_section(Some("version"))
                                .set("time", time.to_rfc3339());
                            ini.write_to_file(&config_path).unwrap();

                            Ok(time)
                        }
                    }
                },
                None => {
                    let prompt_msg = format!("Version publication time is not found in: {}\nWould you like to use the current time as the publication time? (The publication time will be written to version.ini)", config_path.to_string_lossy());
                    let prompt = inquire::Confirm::new(&prompt_msg)
                        .with_default(false)
                        .prompt()?;
                    if !prompt {
                        Err(ConfigKeyMissing("time", config_path.clone(), "This should be the publication time, using the RFC 3339 / ISO 8601 format. E.g. 1996-12-19T16:39:57-08:00"))
                    } else {
                        let time = chrono::Utc::now();

                        let mut ini = ini.clone();

                        ini.with_section(Some("version"))
                            .set("time", time.to_rfc3339());
                        ini.write_to_file(&config_path).unwrap();

                        Ok(time)
                    }
                }
            }
        };
        let time = time?;

        let changelog = read_rtf_or_md_file(&dir.join("CHANGELOG.rtf"))?;

        let sources: Vec<_> = walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) => {
                    let path = entry.into_path();
                    // TODO: Determine sections
                    Some(Source {
                        path,
                        sections: vec![],
                    })
                }
                Err(e) => {
                    println!("Error when scanning sources: {e}");
                    None
                }
            })
            .collect();

        Ok(Self {
            path: dir.into(),
            name: name.into(),
            time,
            changelog,
            sources,
        })
    }
}

#[derive(Debug)]
enum ActionListSection {
    Main,
    MidiEditor,
    MidiInlineeditor,
    MidiEventlisteditor,
    MediaExplorer,
}

impl Into<&str> for ActionListSection {
    fn into(self) -> &'static str {
        match self {
            ActionListSection::Main => "main",
            ActionListSection::MidiEditor => "midi_editor",
            ActionListSection::MidiInlineeditor => "midi_inlineeditor",
            ActionListSection::MidiEventlisteditor => "midi_eventlisteditor",
            ActionListSection::MediaExplorer => "mediaexplorer",
        }
    }
}

impl TryFrom<&str> for ActionListSection {
    type Error = ();

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "main" => Ok(ActionListSection::Main),
            "midi_editor" => Ok(ActionListSection::MidiEditor),
            "midi_inlineeditor" => Ok(ActionListSection::MidiInlineeditor),
            "midi_eventlisteditor" => Ok(ActionListSection::MidiEventlisteditor),
            "mediaexplorer" => Ok(ActionListSection::MediaExplorer),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Source {
    path: PathBuf,
    sections: Vec<ActionListSection>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdata_01() {
        let result = cdata("apple");
        let expected = "<![CDATA[apple]]>";
        assert_eq!(result, expected);
    }

    #[test]
    fn cdata_02() {
        let result = cdata("app]] > < [] &le");
        let expected = "<![CDATA[app]] > < [] &le]]>";
        assert_eq!(result, expected);
    }

    #[test]
    fn cdata_03() {
        let result = cdata("app]]>le");
        let expected = "<![CDATA[app]]]]><![CDATA[>le]]>";
        assert_eq!(result, expected);
    }
}
