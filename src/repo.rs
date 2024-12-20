use anyhow::Result;
use chrono::DateTime;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use itertools::Itertools;
use leon::{Template, Values};
use log::{error, warn};
use once_cell::sync::OnceCell;
use relative_path::{PathExt, RelativePath, RelativePathBuf};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::{self},
    path::{self, Path, PathBuf},
};
use thiserror::Error;
use xml_builder::{XMLBuilder, XMLElement, XMLVersion};

use crate::{
    config::{ActionListSection, PackageConfig, PackageType, RepositoryConfig, VersionConfig},
    templates::{self, PackageTemplateParams},
};

type Entrypoints = HashMap<ActionListSection, GlobSet>;

#[derive(Error, Debug)]
#[error("the given path is not a repository (does not have a repository.toml file): {0}")]
pub(crate) struct NotARepository(PathBuf);

#[derive(Error, Debug)]
#[error("no sources found in package version: `{0}`")]
pub(crate) struct NoSourcesFound(PathBuf);

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

#[derive(Error, Debug)]
#[error("package already exists: `{0}`")]
pub(crate) struct PackageAlreadyExists(PathBuf);

#[derive(Error, Debug)]
#[error("the path is a file: `{0}`")]
pub(crate) struct PathIsAFile(PathBuf);

#[derive(Error, Debug)]
#[error("unable to parse this version string, please specify the new version manually: {0}")]
pub(crate) struct UnknownVersionFormat(String);

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

fn read_txt_file(path: &Path) -> Result<Option<String>> {
    if path.exists() {
        Ok(Some(fs::read_to_string(path)?))
    } else {
        Ok(None)
    }
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
pub(crate) enum GitCommitError {
    #[error("failed to launch git, please ensure it is accessible through the command line")]
    FailedToLaunchGit,
    #[error("failed to get commit hash in the given path: {0}")]
    FailedToGetGitHash(PathBuf),
}

fn get_git_commit(dir: &Path) -> Result<String, GitCommitError> {
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|_| GitCommitError::FailedToLaunchGit)?;

    if !output.status.success() {
        return Err(GitCommitError::FailedToGetGitHash(dir.into()));
    }

    let stdout = String::from_utf8(output.stdout).unwrap();
    let hash = stdout.trim().to_string();

    Ok(hash)
}

fn build_entrypoints(
    patterns_map: &HashMap<ActionListSection, Vec<String>>,
) -> Result<Entrypoints, globset::Error> {
    let mut result: Entrypoints = HashMap::new();

    for (section, patterns) in patterns_map.iter() {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(GlobBuilder::new(pattern).literal_separator(true).build()?);
        }
        let set = builder.build()?;
        result.insert(*section, set);
    }

    Ok(result)
}

#[derive(Debug)]
pub(crate) struct Repository {
    /// Must be an absolute path
    path: PathBuf,
    config: RepositoryConfig,
    git_hash: OnceCell<String>,
}

impl Repository {
    const CONFIG_FILENAME: &'static str = "repository.toml";

    pub(crate) fn read(dir: &Path) -> Result<Self> {
        // convert to absolute path to ensure we can get the folder names etc
        let dir = std::path::absolute(dir).unwrap_or(dir.to_path_buf());

        debug_assert!(
            dir == path::absolute(&dir).unwrap(),
            "dir = {} ; absolute(dir) = {}",
            dir.display(),
            path::absolute(&dir).unwrap().display()
        );

        let config_path = dir.join(Self::CONFIG_FILENAME);
        if !config_path.exists() {
            return Err(NotARepository(dir).into());
        }
        let config: RepositoryConfig = toml::from_str(&fs::read_to_string(&config_path)?)?;

        Ok(Self {
            path: dir,
            config,
            git_hash: OnceCell::new(),
        })
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

    pub(crate) fn url_pattern(&self) -> &str {
        &self.config.url_pattern
    }

    pub(crate) fn git_hash(&self) -> Result<&str, GitCommitError> {
        self.git_hash
            .get_or_try_init(|| get_git_commit(&self.path))
            .map(|x| x.as_str())
    }

    pub(crate) fn packages(&self) -> Result<Vec<Package>> {
        Package::discover_packages(self.path())
    }

    pub(crate) fn add_package(&self, identifier: &str) -> Result<Package> {
        let existing_packages = self.packages()?;
        if let Some(pkg) = existing_packages
            .iter()
            .find(|pkg| pkg.identifier() == identifier)
        {
            return Err(PackageAlreadyExists(pkg.path().into()).into());
        }

        let target_path = {
            let base_path = self.path().join(identifier);
            if !base_path.exists() {
                base_path
            } else {
                let mut i = 1;
                loop {
                    let numbered_path = self.path().join(format!("{}_{}", identifier, i));
                    if !numbered_path.exists() {
                        break numbered_path;
                    }
                    i += 1;
                }
            }
        };

        // TODO: Allow specifying config when creating package
        Package::create_package(&target_path, None)
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
            for pkg in packages.into_iter() {
                if !pkg_map.contains_key(pkg.category()) {
                    pkg_map.insert(pkg.category().to_relative_path_buf(), vec![]);
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
                let reapack = pkg.element(self)?;
                category.add_child(reapack).unwrap();
            }

            index.add_child(category).unwrap();
        }

        Ok(index)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Package {
    path: PathBuf,
    config: PackageConfig,
    entrypoints: OnceCell<Option<Entrypoints>>,
}

impl Package {
    const CONFIG_FILENAME: &'static str = "package.toml";

    pub(crate) fn read(dir: &Path) -> Result<Self> {
        debug_assert!(
            dir == path::absolute(&dir).unwrap(),
            "dir = {} ; absolute(dir) = {}",
            dir.display(),
            path::absolute(&dir).unwrap().display()
        );

        let config_path = dir.join(Self::CONFIG_FILENAME);
        let config: PackageConfig = toml::from_str(&fs::read_to_string(config_path)?)?;

        Ok(Self {
            path: dir.into(),
            config,
            entrypoints: OnceCell::new(),
        })
    }

    pub(crate) fn identifier(&self) -> Cow<str> {
        if let Some(identifier) = &self.config.identifier {
            identifier.into()
        } else {
            self.path.file_name().unwrap().to_string_lossy()
        }
    }

    pub(crate) fn name(&self) -> Cow<str> {
        if let Some(name) = &self.config.name {
            name.into()
        } else {
            self.identifier()
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn category(&self) -> &RelativePath {
        &self.config.category
    }

    pub(crate) fn pkg_type(&self) -> PackageType {
        self.config.pkg_type.clone()
    }

    pub(crate) fn author(&self) -> Option<&str> {
        self.config.author.as_deref()
    }

    pub(crate) fn readme(&self) -> Result<Option<String>> {
        read_rtf_or_md_file(&self.path.join("README.rtf"))
    }

    pub(crate) fn entrypoints(&self) -> Result<Option<&Entrypoints>, globset::Error> {
        self.entrypoints
            .get_or_try_init(|| match &self.config.entrypoints {
                Some(patterns_map) => build_entrypoints(patterns_map).map(Some),
                None => Ok(None),
            })
            .map(|x| x.as_ref())
    }

    pub(crate) fn versions(&self) -> Result<Vec<Version>> {
        Version::discover_versions(self.path())
    }

    pub(crate) fn latest_version(&self) -> Result<Option<Version>> {
        Ok(self
            .versions()?
            .iter()
            .max_by(|a, b| Version::compare_version_names(&a.name(), &b.name()))
            .cloned())
    }

    fn create_package(path: &Path, config: Option<PackageTemplateParams>) -> Result<Package> {
        let path = path::absolute(path)?;

        // check if package already exists
        if path.exists() {
            let metadata = path.metadata()?;
            if !metadata.is_dir() {
                return Err(PathIsAFile(path.into()).into());
            }
        } else {
            fs::create_dir(&path)?;
        }
        let config_path = path.join(Self::CONFIG_FILENAME);
        if config_path.exists() {
            return Err(PackageAlreadyExists(path.into()).into());
        }

        // create package config
        let config_text =
            templates::generate_package_config(&config.unwrap_or(PackageTemplateParams::default()));
        fs::write(&config_path, config_text)?;

        // read the package
        Self::read(&path)
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

    fn element(&self, repo: &Repository) -> Result<XMLElement> {
        let mut reapack = XMLElement::new("reapack");
        reapack.add_attribute("desc", &self.name());
        reapack.add_attribute("type", (&self.pkg_type()).into());
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
            reapack.add_child(version.element(repo, self)?).unwrap();
        }

        Ok(reapack)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Version {
    path: PathBuf,
    config: VersionConfig,
    entrypoints: OnceCell<Option<Entrypoints>>,
}

impl Version {
    const CONFIG_FILENAME: &'static str = "version.toml";

    /// Splits version names by dots '.', then compares each segment.
    pub(crate) fn compare_version_names(version_a: &str, version_b: &str) -> std::cmp::Ordering {
        for entry in version_a.split('.').zip_longest(version_b.split('.')) {
            match entry {
                itertools::EitherOrBoth::Both(part_a, part_b) => match part_a
                    .partial_cmp(part_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                {
                    // comparison is equal, don't return, keep iterating
                    std::cmp::Ordering::Equal => (),
                    // otherwise, return that order (greater/less)
                    order => return order,
                },
                // if one version is longer, return that one
                itertools::EitherOrBoth::Left(_part_a) => return std::cmp::Ordering::Greater,
                itertools::EitherOrBoth::Right(_part_b) => return std::cmp::Ordering::Less,
            };
        }
        std::cmp::Ordering::Equal
    }

    pub(crate) fn increment_version(text: &str) -> Result<String, UnknownVersionFormat> {
        let text = text.to_string();

        let suffix = {
            let mut suffix = String::new();
            for c in text.chars().rev() {
                // if found non-digit char, stop the loop
                if c.is_ascii_digit() {
                    suffix.push(c);
                } else {
                    break;
                }
            }
            if suffix.is_empty() {
                return Err(UnknownVersionFormat(text));
            }
            suffix = suffix.chars().rev().collect();
            Ok(suffix)
        }?;

        // Parse the suffix to an integer
        let incremented_suffix = suffix.parse::<u32>().unwrap() + 1;

        // Create the new version string
        let prefix_len = text.len() - suffix.len();

        Ok(format!("{}{}", &text[..prefix_len], incremented_suffix))
    }

    pub(crate) fn read(dir: &Path) -> Result<Self> {
        debug_assert!(
            dir == path::absolute(&dir).unwrap(),
            "dir = {} ; absolute(dir) = {}",
            dir.display(),
            path::absolute(&dir).unwrap().display()
        );

        let config_path = dir.join(Self::CONFIG_FILENAME);
        let config: VersionConfig = toml::from_str(&fs::read_to_string(config_path)?)?;

        Ok(Self {
            path: dir.into(),
            config,
            entrypoints: OnceCell::new(),
        })
    }

    pub(crate) fn name(&self) -> Cow<str> {
        self.path.file_name().unwrap().to_string_lossy()
    }

    pub(crate) fn time(&self) -> DateTime<chrono::Utc> {
        self.config.time
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn changelog(&self) -> Result<Option<String>> {
        read_txt_file(&self.path.join("CHANGELOG.txt"))
    }

    pub(crate) fn entrypoints<'a>(
        &'a self,
        pkg: &'a Package,
    ) -> Result<Option<&'a Entrypoints>, globset::Error> {
        let entrypoints = self
            .entrypoints
            .get_or_try_init(|| match &self.config.entrypoints {
                Some(patterns_map) => build_entrypoints(patterns_map).map(Some),
                None => Ok(None),
            })?;
        if entrypoints.is_some() {
            return Ok(entrypoints.as_ref());
        }

        pkg.entrypoints()
    }

    pub(crate) fn sources(&self) -> Result<Vec<Source>, NoSourcesFound> {
        Source::discover_sources(&self.path)
    }

    fn discover_versions(dir: &Path) -> Result<Vec<Version>> {
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

            let pkg = match Version::read(&path) {
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

    fn element(&self, repo: &Repository, pkg: &Package) -> Result<XMLElement> {
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
        let sources = self.sources()?;
        for source in sources.iter() {
            version.add_child(source.element(repo, pkg, self)?).unwrap();
        }

        // for script packages, check there is at least one entrypoint
        {
            let pkg_type = pkg.pkg_type();
            if pkg_type == PackageType::Script {
                let mut package_has_no_entrypoints = true;
                for src in sources {
                    let sections = src.sections(pkg, self)?;
                    if !sections.is_empty() {
                        package_has_no_entrypoints = false;
                        break;
                    }
                }
                if package_has_no_entrypoints {
                    return Err(NoEntrypointsFoundForScriptPackage(pkg.path().into()).into());
                }
            }
        }

        Ok(version)
    }
}

struct UrlTemplateValueProvider<'a> {
    repo: &'a Repository,
    pkg: &'a Package,
    ver: &'a Version,
    src: &'a Source,
}

impl Values for UrlTemplateValueProvider<'_> {
    fn get_value(&self, key: &str) -> Option<Cow<'_, str>> {
        match key {
            "git_commit" => match self.repo.git_hash() {
                Ok(hash) => Some(hash.into()),
                Err(err) => {
                    error!("failed to obtain URL variable `git_commit` due to {err}");
                    None
                }
            },
            "relpath" => {
                // path of source, relative to root of repository
                let source_relpath = self.src.path().relative_to(self.repo.path()).unwrap();
                let encoded_path = url_encode_path(&source_relpath);
                Some(encoded_path.into())
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Source {
    path: PathBuf,
    sections: OnceCell<HashSet<ActionListSection>>,
}

impl Source {
    fn read(path: &Path) -> Self {
        debug_assert!(
            path == path::absolute(&path).unwrap(),
            "path = {} ; absolute(path) = {}",
            path.display(),
            path::absolute(&path).unwrap().display()
        );

        Self {
            path: path.into(),
            sections: OnceCell::new(),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn url(&self, repo: &Repository, pkg: &Package, ver: &Version) -> Result<String> {
        let url_pattern = repo.url_pattern();
        // TODO: Find a way to not parse a new template from scratch for every source
        let template = Template::parse(url_pattern)?;
        let values = UrlTemplateValueProvider {
            repo,
            pkg,
            ver,
            src: self,
        };

        Ok(template.render(&values)?)
    }

    /// The relative path of this source file from its version folder.
    ///
    /// E.g. An absolute path like `"C:/index/my-package/0.0.1/foo/index.lua"` will return `"foo/index.lua"`
    fn relpath_from_version(&self, ver: &Version) -> RelativePathBuf {
        self.path.relative_to(ver.path()).unwrap()
    }

    /// The desired output path of this source file, relative to the root of a folder. E.g. `"my-package/foo/index.lua"`
    ///
    /// Note: This does NOT consider the subfolders created by the package category. Use [Source::output_relpath_from_category] instead.
    fn output_relpath(&self, pkg: &Package, ver: &Version) -> RelativePathBuf {
        let result = RelativePathBuf::from_path(pkg.identifier().as_ref())
            .expect("package identifier cannot be an absolute path")
            .join(self.relpath_from_version(ver));
        debug_assert!(result == result.normalize());
        result
    }

    fn discover_sources(dir: &Path) -> Result<Vec<Source>, NoSourcesFound> {
        let sources: Vec<_> = walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) => {
                    let path = entry.path();

                    // skip directories
                    match entry.metadata() {
                        Err(e) => {
                            warn!(
                                "failed to get metadata for source {} due to {}",
                                path.display(),
                                e
                            );
                            return None;
                        }
                        Ok(metadata) => {
                            if !metadata.is_file() {
                                return None;
                            }
                        }
                    }

                    Some(Source::read(path))
                }
                Err(e) => {
                    warn!("failed to read source {}", e);
                    None
                }
            })
            .collect();

        if sources.is_empty() {
            Err(NoSourcesFound(dir.into()))
        } else {
            Ok(sources)
        }
    }

    /// The 'file' attribute of the Element. A relative path from the Category folder to the source's target location. E.g. `"../my-package/foo/index.lua"`
    fn output_relpath_from_category(&self, pkg: &Package, ver: &Version) -> RelativePathBuf {
        let mut result = RelativePathBuf::new();
        // prepend '..' for each segment in category
        for component in pkg.category().components() {
            match component {
                relative_path::Component::CurDir => (),
                relative_path::Component::ParentDir => panic!(
                    "package category cannot refer to the parent directory {}",
                    pkg.category()
                ),
                relative_path::Component::Normal(_) => result.push(".."),
            }
        }
        // push the normal expected output path
        result.push(&self.output_relpath(pkg, ver));
        result
    }

    fn element(&self, repo: &Repository, pkg: &Package, ver: &Version) -> Result<XMLElement> {
        let mut source = XMLElement::new("source");
        source.add_text(self.url(repo, pkg, ver)?).unwrap();
        source.add_attribute("file", self.output_relpath_from_category(pkg, ver).as_ref());

        // TODO: Implement setting "type" attribute
        // https://github.com/cfillion/reapack/wiki/Index-Format#source-element

        let sections = self.sections(pkg, ver)?;

        if !sections.is_empty() {
            let sections = sections.iter().map(Into::<&str>::into).join(" ");
            source.add_attribute("main", &sections);
        }

        Ok(source)
    }

    fn sections(&self, pkg: &Package, ver: &Version) -> Result<&HashSet<ActionListSection>> {
        self.sections.get_or_try_init(|| {
            let entrypoints = ver.entrypoints(pkg)?;
            let pkg_type = pkg.pkg_type();
            if pkg_type == PackageType::Script {
                let Some(entrypoints) = entrypoints else {
                    return Err(NoEntrypointsDefinedForScriptPackage(pkg.path().into()).into());
                };
                if entrypoints.iter().all(|(_, pattern)| pattern.is_empty()) {
                    return Err(NoEntrypointsDefinedForScriptPackage(pkg.path().into()).into());
                }
            } else if let Some(entrypoints) = entrypoints {
                if entrypoints.iter().any(|(_, pattern)| !pattern.is_empty()) {
                    return Err(EntrypointsOnlyAllowedInScriptPackages(pkg.path().into()).into());
                }
            }
            let relpath_to_ver = self.relpath_from_version(ver);
            let sections = match entrypoints {
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
            Ok(sections)
        })
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
