use std::{collections::HashMap, path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use relative_path::RelativePathBuf;
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// As defined in:
/// https://github.com/cfillion/reapack/blob/master/src/package.cpp#L36
#[derive(Debug, Clone)]
pub(crate) enum PackageType {
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

#[derive(PartialEq, Eq, Hash, Debug)]
pub(crate) enum ActionListSection {
    MainSection,                // main
    MIDIEditorSection,          // midi_editor
    MIDIInlineEditorSection,    // midi_inlineeditor
    MIDIEventListEditorSection, // midi_eventlisteditor
    MediaExplorerSection,       // mediaexplorer
}

#[derive(Error, Debug)]
#[error("invalid action list section: {0}")]
pub(crate) struct InvalidActionListSection(String);

impl FromStr for ActionListSection {
    type Err = InvalidActionListSection;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "main" => Ok(Self::MainSection),
            "midi_editor" => Ok(Self::MIDIEditorSection),
            "midi_inlineeditor" => Ok(Self::MIDIInlineEditorSection),
            "midi_eventlisteditor" => Ok(Self::MIDIEventListEditorSection),
            "mediaexplorer" => Ok(Self::MediaExplorerSection),
            _ => Err(InvalidActionListSection(s.into())),
        }
    }
}

impl From<&ActionListSection> for &str {
    fn from(value: &ActionListSection) -> Self {
        match value {
            ActionListSection::MainSection => "main",
            ActionListSection::MIDIEditorSection => "midi_editor",
            ActionListSection::MIDIInlineEditorSection => "midi_inlineeditor",
            ActionListSection::MIDIEventListEditorSection => "midi_eventlisteditor",
            ActionListSection::MediaExplorerSection => "mediaexplorer",
        }
    }
}

impl Serialize for ActionListSection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.into())
    }
}

impl<'de> Deserialize<'de> for ActionListSection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // let x = String::deserialize(deserializer)?;
        let text = String::deserialize(deserializer)?;
        text.parse::<ActionListSection>()
            .map_err(|e| serde::de::Error::custom(e))
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RepositoryConfig {
    pub(crate) identifier: Option<String>,
    pub(crate) author: String,
    pub(crate) url_pattern: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PackageConfig {
    pub(crate) name: Option<String>,
    pub(crate) category: RelativePathBuf,
    pub(crate) r#type: PackageType,
    pub(crate) identifier: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) entrypoints: Option<HashMap<ActionListSection, Vec<RelativePathBuf>>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct VersionConfig {
    pub(crate) time: DateTime<Utc>,
    pub(crate) entrypoints: Option<HashMap<ActionListSection, Vec<RelativePathBuf>>>,
}
