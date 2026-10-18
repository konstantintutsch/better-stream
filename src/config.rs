use serde::Deserialize;
use std::{fs, io, path::PathBuf};

use crate::stream::rtsp;

const FILE: &str = "better-stream/config.toml";

/// User supplied settings and list of RTSP sources, read from a TOML file
#[derive(Default, Debug, Clone, Deserialize)]
pub struct Config {
    /// Hide the window title bar, which also shows the quit button (default: false)
    #[serde(default)]
    pub hide_title_bar: bool,
    #[serde(default)]
    pub sources: Vec<rtsp::Source>,
}

impl Config {
    /// Location of the configuration file
    pub fn path() -> PathBuf {
        match dirs::config_dir() {
            Some(directory) => directory.join(FILE),
            None => PathBuf::from(FILE),
        }
    }

    /// Read the configuration file, falling back to an empty configuration
    pub fn load() -> Self {
        let path = Self::path();

        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    log::error!("Failed to read {}: {error}", path.display());
                }
                return Self::default();
            }
        };

        match toml::from_str(&contents) {
            Ok(config) => config,
            Err(error) => {
                log::error!("Failed to parse {}: {error}", path.display());
                Self::default()
            }
        }
    }
}
