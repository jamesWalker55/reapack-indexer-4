use anyhow::Result;
use chrono::DateTime;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use itertools::Itertools;
use log::{debug, error, info, trace, warn};
use relative_path::{PathExt, RelativePath, RelativePathBuf};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::{self},
    path::{Path, PathBuf},
};
use thiserror::Error;
use xml_builder::{XMLBuilder, XMLElement, XMLVersion};

use crate::config::{
    ActionListSection, PackageConfig, PackageType, RepositoryConfig, VersionConfig,
};

type Entrypoints = HashMap<ActionListSection, GlobSet>;

#[derive(Error, Debug)]
#[error("the given path is not a repository (does not have a repository.toml file): {0}")]
pub(crate) struct NotARepository(PathBuf);

#[derive(Error, Debug)]
#[error("unknown variable in URL pattern: `{0}`")]
pub(crate) struct UnknownURLPatternVariable(String);

#[derive(Error, Debug)]
#[error("entrypoints can only be defined in packages with type = \"script\": `{0}`")]
pub(crate) struct EntrypointsOnlyAllowedInScriptPackages(PathBuf);

#[derive(Error, Debug)]
#[error("script packages must have entrypoints defined: `{0}`")]
pub(crate) struct NoEntrypointsDefinedForScriptPackage(PathBuf);

#[derive(Error, Debug)]
#[error("entrypoints is defined in config, but no files were matched: `{0}`")]
pub(crate) struct NoEntrypointsFoundForScriptPackage(PathBuf);

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

fn url_encode_path(path: &RelativePath) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

    let input = path.normalize().to_string();

    const FRAGMENT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'/')
        .remove(b'.')
        .remove(b'-')
        .remove(b'_');
    utf8_percent_encode(&input, FRAGMENT).to_string()
}

#[derive(Error, Debug)]
#[error("failed to launch git, please ensure it is accessible through the command line")]
struct FailedToLaunchGit;

#[derive(Error, Debug)]
#[error("failed to get commit hash in the given path: {0}")]
struct FailedToGetGitHash(PathBuf);

fn get_git_commit(dir: &Path) -> Result<String> {
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|_| FailedToLaunchGit)?;

    if !output.status.success() {
        return Err(FailedToGetGitHash(dir.into()).into());
    }

    let stdout = String::from_utf8(output.stdout).unwrap();
    let hash = stdout.trim().to_string();

    Ok(hash)
}

#[derive(Debug)]
pub(crate) struct Repo {
    /// Must be an absolute path
    path: PathBuf,
    config: RepositoryConfig,
}

impl Repo {
    const CONFIG_FILENAME: &'static str = "repository.toml";

    pub(crate) fn read(dir: &Path) -> Result<Self> {
        // convert to absolute path to ensure we can get the folder names etc
        let dir = std::path::absolute(dir).unwrap_or(dir.to_path_buf());

        let config_path = dir.join(Self::CONFIG_FILENAME);
        if !config_path.exists() {
            return Err(NotARepository(dir).into());
        }
        let config: RepositoryConfig = toml::from_str(&fs::read_to_string(&config_path)?)?;

        Ok(Self { path: dir, config })
    }

    /// Unique identifier for this repo.
    /// Will be used as the folder name to store the repo.
    pub(crate) fn identifier(&self) -> Cow<str> {
        if let Some(identifier) = self.config.identifier.as_ref() {
            identifier.into()
        } else {
            self.path.file_name().unwrap().to_string_lossy()
        }
    }

    pub(crate) fn readme(&self) -> Result<Option<String>> {
        read_rtf_or_md_file(&self.path.join("README.rtf"))
    }

    pub(crate) fn path(&self) -> &Path {
        self.path.as_ref()
    }

    pub(crate) fn author(&self) -> &str {
        &self.config.author
    }

    pub(crate) fn packages(&self) -> Result<Vec<Package>> {
        Package::discover_packages(self.path())
    }

    pub(crate) fn generate_index(&self) -> Result<String> {
        let mut xml = XMLBuilder::new()
            .version(XMLVersion::XML1_1)
            .encoding("UTF-8".into())
            .build();

        let root_element = self.element()?;
        xml.set_root_element(root_element);

        let mut buf: Vec<u8> = Vec::new();
        xml.generate(&mut buf)?;
        let result = String::from_utf8(buf)?;

        Ok(result)
    }

    fn element(&self) -> Result<XMLElement> {
        let mut index = XMLElement::new("index");
        index.add_attribute("version", "1");
        index.add_attribute("name", &self.identifier());

        // add description
        if let Some(desc) = &self.readme()? {
            let mut metadata = XMLElement::new("metadata");
            let mut description = XMLElement::new("description");
            description.add_text(cdata(desc)).unwrap();
            metadata.add_child(description).unwrap();
            index.add_child(metadata).unwrap();
        }

        // group packages into categories
        let packages = self.packages()?;
        let pkg_map = {
            let mut pkg_map = HashMap::new();
            for pkg in packages.iter() {
                if !pkg_map.contains_key(pkg.category()) {
                    pkg_map.insert(pkg.category(), vec![]);
                }
                let packages = pkg_map.get_mut(pkg.category()).unwrap();
                packages.push(pkg)
            }
            pkg_map
        };

        // insert categories into index
        for (category_name, packages) in pkg_map.iter() {
            let mut category = XMLElement::new("category");
            category.add_attribute("name", category_name.as_ref());

            for pkg in packages {
                let reapack = pkg.element(&self)?;
                category.add_child(reapack).unwrap();
            }

            index.add_child(category).unwrap();
        }

        Ok(index)
    }
}

#[derive(Debug)]
pub(crate) struct Package {
    path: PathBuf,
    config: PackageConfig,
}

impl Package {
    const CONFIG_FILENAME: &'static str = "package.toml";

    pub(crate) fn read(dir: &Path) -> Result<Self> {
        let config_path = dir.join(Self::CONFIG_FILENAME);
        let config: PackageConfig = toml::from_str(&fs::read_to_string(&config_path)?)?;

        Ok(Self {
            path: dir.into(),
            config,
        })
    }

    fn identifier(&self) -> Cow<str> {
        if let Some(identifier) = &self.config.identifier {
            identifier.into()
        } else {
            self.path.file_name().unwrap().to_string_lossy()
        }
    }

    fn name(&self) -> Cow<str> {
        if let Some(name) = &self.config.name {
            name.into()
        } else {
            self.identifier()
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn category(&self) -> &RelativePath {
        &self.config.category
    }

    fn r#type(&self) -> PackageType {
        self.config.r#type.clone()
    }

    fn author(&self) -> Option<&str> {
        self.config.author.as_ref().map(|x| x.as_str())
    }

    fn readme(&self) -> Result<Option<String>> {
        read_rtf_or_md_file(&self.path.join("README.rtf"))
    }

    pub(crate) fn versions(&self) -> Result<Vec<PackageVersion>> {
        PackageVersion::discover_versions(self.path())
    }

    fn discover_packages(dir: &Path) -> Result<Vec<Package>> {
        let mut result = vec![];
        for entry in fs::read_dir(dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warn!("failed to read entry {}", err);
                    continue;
                }
            };
            let path = entry.path();

            let is_dir = match path.metadata() {
                Ok(metadata) => metadata.is_dir(),
                Err(err) => {
                    warn!(
                        "failed to get metadata for entry {} due to {}",
                        path.display(),
                        err
                    );
                    continue;
                }
            };
            if !is_dir {
                continue;
            }
            if !path.join(Self::CONFIG_FILENAME).exists() {
                continue;
            }

            let pkg = match Package::read(&path) {
                Ok(pkg) => pkg,
                Err(err) => {
                    warn!("failed to read package {} due to {}", path.display(), err);
                    continue;
                }
            };
            result.push(pkg);
        }
        Ok(result)
    }

    fn element(&self, repo: &Repo) -> Result<XMLElement> {
        let mut reapack = XMLElement::new("reapack");
        reapack.add_attribute("desc", &self.name());
        reapack.add_attribute("type", (&self.r#type()).into());
        reapack.add_attribute("name", &self.identifier());

        // add description
        if let Some(desc) = &self.readme()? {
            let mut metadata = XMLElement::new("metadata");
            let mut description = XMLElement::new("description");
            description.add_text(cdata(desc)).unwrap();
            metadata.add_child(description).unwrap();
            reapack.add_child(metadata).unwrap();
        }

        // add versions
        for version in self.versions()?.iter() {
            reapack.add_child(version.element(&repo, &self)?).unwrap();
        }

        Ok(reapack)
    }
}

#[derive(Debug)]
pub(crate) struct PackageVersion {
    path: PathBuf,
    config: VersionConfig,
}

impl PackageVersion {
    const CONFIG_FILENAME: &'static str = "version.toml";

    pub(crate) fn read(dir: &Path) -> Result<Self> {
        let config_path = dir.join(Self::CONFIG_FILENAME);
        let config: VersionConfig = toml::from_str(&fs::read_to_string(&config_path)?)?;

        Ok(Self {
            path: dir.into(),
            config,
        })
    }

    pub(crate) fn name(&self) -> Cow<str> {
        self.path.file_name().unwrap().to_string_lossy()
    }

    pub(crate) fn time(&self) -> DateTime<chrono::Utc> {
        self.config.time
    }

    pub(crate) fn changelog(&self) -> Result<Option<String>> {
        read_rtf_or_md_file(&self.path.join("CHANGELOG.rtf"))
    }

    pub(crate) fn sources(&self) -> Result<Vec<Source>> {
        todo!()
    }

    fn discover_versions(dir: &Path) -> Result<Vec<PackageVersion>> {
        let mut result = vec![];
        for entry in fs::read_dir(dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warn!("failed to read entry {}", err);
                    continue;
                }
            };
            let path = entry.path();

            let is_dir = match path.metadata() {
                Ok(metadata) => metadata.is_dir(),
                Err(err) => {
                    warn!(
                        "failed to get metadata for entry {} due to {}",
                        path.display(),
                        err
                    );
                    continue;
                }
            };
            if !is_dir {
                continue;
            }
            if !path.join(Self::CONFIG_FILENAME).exists() {
                continue;
            }

            let pkg = match PackageVersion::read(&path) {
                Ok(pkg) => pkg,
                Err(err) => {
                    warn!("failed to read version {} due to {}", path.display(), err);
                    continue;
                }
            };
            result.push(pkg);
        }
        Ok(result)
    }

    fn element(&self, repo: &Repo, pkg: &Package) -> Result<XMLElement> {
        let mut version = XMLElement::new("version");
        version.add_attribute("name", &self.name());
        version.add_attribute("author", pkg.author().unwrap_or(repo.author()));
        version.add_attribute("time", &self.time().to_rfc3339());

        // add changelog
        if let Some(text) = &self.changelog()? {
            let mut changelog = XMLElement::new("changelog");
            changelog.add_text(cdata(text)).unwrap();
            version.add_child(changelog).unwrap();
        }

        // add sources
        for source in self.sources()?.iter() {
            version.add_child(source.element()).unwrap();
        }

        Ok(version)
    }
}

#[derive(Debug)]
pub(crate) struct Source {
    /// ReaPack's 'file' path, relative to the generated category folder path
    output_relpath: RelativePathBuf,
    sections: HashSet<ActionListSection>,
    url: String,
}

struct SourceParams<'a> {
    repo_path: &'a Path,
    version_path: &'a Path,
    category: &'a RelativePath,
    url_pattern: &'a str,
    entrypoints: &'a Option<&'a Entrypoints>,
}

impl Source {
    fn read(path: &Path, params: SourceParams) -> Result<Self> {
        // path of source file relative to repository root
        let relpath_to_repo = path.relative_to(params.repo_path)?;
        // path of source file relative to version root
        let relpath_to_ver = path.relative_to(params.version_path)?;

        let sections = match params.entrypoints {
            Some(entrypoints) => entrypoints
                .iter()
                .filter_map(|(section, globset)| {
                    // Use '.to_string()' instead of '.to_path(".")'!!
                    // Because '.to_path(".")' adds a './' to the beginning of the path, messing up the glob matcher,
                    // while '.to_string()' does not add a './' and keeps the path as-is.
                    let matches = globset.matches(relpath_to_ver.to_string());
                    if matches.is_empty() {
                        None
                    } else {
                        Some(*section)
                    }
                })
                .collect(),
            None => HashSet::new(),
        };

        let url_pattern = Self::apply_url_pattern(&relpath_to_repo, params.url_pattern)?;
        let variable_regex = regex::Regex::new(r"\{.*?\}").unwrap();
        if let Some(cap) = variable_regex.captures(&url_pattern) {
            let mat = cap.get(0).unwrap();
            return Err(UnknownURLPatternVariable(mat.as_str().to_string()).into());
        }

        let output_path = {
            let category_path = params.category;
            category_path.relative(&relpath_to_repo)
        };

        Ok(Self {
            output_relpath: output_path,
            // TODO: Determine sections
            sections,
            url: url_pattern,
        })
    }

    fn apply_url_pattern(path: &RelativePath, pattern: &str) -> Result<String> {
        let mut pattern = pattern.to_string();

        if pattern.contains("{relpath}") {
            let path = url_encode_path(path);
            pattern = pattern.replace("{relpath}", &path);
        }

        Ok(pattern)
    }

    fn element(&self) -> XMLElement {
        let mut source = XMLElement::new("source");
        source.add_text(self.url.clone()).unwrap();
        source.add_attribute("file", self.output_relpath.as_ref());

        // TODO: Implement setting "type" attribute
        // https://github.com/cfillion/reapack/wiki/Index-Format#source-element

        if !self.sections.is_empty() {
            let sections = self.sections.iter().map(Into::<&str>::into).join(" ");
            source.add_attribute("main", &sections);
        }

        source
    }
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
