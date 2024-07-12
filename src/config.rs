use std::{path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use relative_path::RelativePathBuf;
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// As defined in:
/// https://github.com/cfillion/reapack/blob/master/src/package.cpp#L36
enum PackageType {
    Script,          // script
    Extension,       // extension
    Effect,          // effect
    Data,            // data
    Theme,           // theme
    LangPack,        // langpack
    WebInterface,    // webinterface
    ProjectTemplate, // projecttpl
    TrackTemplate,   // tracktpl
    MIDINoteNames,   // midinotenames
    AutomationItem,  // autoitem
}

#[derive(Error, Debug)]
#[error("invalid package type: {0}")]
pub(crate) struct InvalidPackageType(String);

impl FromStr for PackageType {
    type Err = InvalidPackageType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "script" => Ok(Self::Script),
            "extension" => Ok(Self::Extension),
            "effect" => Ok(Self::Effect),
            "data" => Ok(Self::Data),
            "theme" => Ok(Self::Theme),
            "langpack" => Ok(Self::LangPack),
            "webinterface" => Ok(Self::WebInterface),
            "projecttpl" => Ok(Self::ProjectTemplate),
            "tracktpl" => Ok(Self::TrackTemplate),
            "midinotenames" => Ok(Self::MIDINoteNames),
            "autoitem" => Ok(Self::AutomationItem),
            _ => Err(InvalidPackageType(s.into())),
        }
    }
}

impl From<&PackageType> for &str {
    fn from(value: &PackageType) -> Self {
        match value {
            PackageType::Script => "script",
            PackageType::Extension => "extension",
            PackageType::Effect => "effect",
            PackageType::Data => "data",
            PackageType::Theme => "theme",
            PackageType::LangPack => "langpack",
            PackageType::WebInterface => "webinterface",
            PackageType::ProjectTemplate => "projecttpl",
            PackageType::TrackTemplate => "tracktpl",
            PackageType::MIDINoteNames => "midinotenames",
            PackageType::AutomationItem => "autoitem",
        }
    }
}

impl Serialize for PackageType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.into())
    }
}

impl<'de> Deserialize<'de> for PackageType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // let x = String::deserialize(deserializer)?;
        let text = String::deserialize(deserializer)?;
        text.parse::<PackageType>()
            .map_err(|e| serde::de::Error::custom(e))
    }
}

#[derive(Serialize, Deserialize)]
struct RepositoryConfig {
    identifier: Option<String>,
    author: String,
    url_pattern: String,
}

#[derive(Serialize, Deserialize)]
struct PackageConfig {
    name: String,
    category: RelativePathBuf,
    r#type: PackageType,
    identifier: Option<String>,
    author: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct VersionConfig {
    time: DateTime<Utc>,
}
