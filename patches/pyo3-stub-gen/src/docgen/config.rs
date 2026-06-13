//! Configuration for documentation generation

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for documentation generation from pyproject.toml
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocGenConfig {
    /// Output directory for generated documentation
    #[serde(rename = "output-dir", default = "default_output_dir")]
    pub output_dir: PathBuf,

    /// Name of the JSON output file
    #[serde(rename = "json-output", default = "default_json_output")]
    pub json_output: String,

    /// Generate separate .rst pages for each module (default: true)
    #[serde(rename = "separate-pages", default = "default_separate_pages")]
    pub separate_pages: bool,

    /// Custom intro message for index.rst (default: standard message, empty string to omit)
    #[serde(rename = "intro-message", default)]
    pub intro_message: Option<String>,

    /// Custom title for index.rst (default: "{package_name} API Reference", empty string to use "API Reference")
    #[serde(rename = "index-title", default)]
    pub index_title: Option<String>,

    /// Generate module contents tables (default: false)
    #[serde(rename = "contents-table", default)]
    pub contents_table: bool,
}

impl Default for DocGenConfig {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            json_output: default_json_output(),
            separate_pages: default_separate_pages(),
            intro_message: None,
            index_title: None,
            contents_table: false,
        }
    }
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("docs/api")
}

fn default_json_output() -> String {
    "api_reference.json".to_string()
}

fn default_separate_pages() -> bool {
    true
}

impl DocGenConfig {
    /// Convert output_dir to relative POSIX path for JSON serialization
    pub fn to_relative_posix_path(&self, base_dir: &std::path::Path) -> String {
        let relative_path = if self.output_dir.is_absolute() {
            self.output_dir
                .strip_prefix(base_dir)
                .unwrap_or(&self.output_dir)
        } else {
            &self.output_dir
        };

        // Convert to POSIX format (forward slashes)
        relative_path
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}
