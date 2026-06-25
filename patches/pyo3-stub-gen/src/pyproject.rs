//! `pyproject.toml` parser for reading `[tool.maturin]` configuration.
//!
//! ```
//! use pyo3_stub_gen::pyproject::PyProject;
//! use std::path::Path;
//!
//! let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
//! let pyproject = PyProject::parse_toml(
//!     root.join("examples/mixed/pyproject.toml")
//! ).unwrap();
//! ```

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::*};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PyProject {
    pub project: Project,
    pub tool: Option<Tool>,

    #[serde(skip)]
    toml_path: PathBuf,
}

impl PyProject {
    pub fn parse_toml(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.file_name() != Some("pyproject.toml".as_ref()) {
            bail!("{} is not a pyproject.toml", path.display())
        }
        let mut out: PyProject = toml::de::from_str(&fs::read_to_string(path)?)?;
        out.toml_path = path.to_path_buf();
        Ok(out)
    }

    pub fn module_name(&self) -> &str {
        if let Some(tool) = &self.tool {
            if let Some(maturin) = &tool.maturin {
                if let Some(module_name) = &maturin.module_name {
                    return module_name;
                }
            }
        }
        &self.project.name
    }

    /// Return `tool.maturin.python_source` if it exists, which means the project is a mixed Rust/Python project.
    pub fn python_source(&self) -> Option<PathBuf> {
        if let Some(tool) = &self.tool {
            if let Some(maturin) = &tool.maturin {
                if let Some(python_source) = &maturin.python_source {
                    if let Some(base) = self.toml_path.parent() {
                        return Some(base.join(python_source));
                    } else {
                        return Some(PathBuf::from(python_source));
                    }
                }
            }
        }
        None
    }

    /// Return stub generation configuration from `[tool.pyo3-stub-gen]`.
    /// Returns default configuration if the section is not present.
    pub fn stub_gen_config(&self) -> StubGenConfig {
        self.tool
            .as_ref()
            .and_then(|t| t.pyo3_stub_gen.clone())
            .unwrap_or_default()
    }

    /// Return doc-gen configuration with output_dir resolved relative to pyproject.toml directory
    pub fn doc_gen_config_resolved(&self) -> Option<crate::docgen::DocGenConfig> {
        if let Some(mut config) = self.stub_gen_config().doc_gen {
            // Resolve output_dir relative to pyproject.toml directory
            // Only resolve if the path is relative (absolute paths stay unchanged)
            if config.output_dir.is_relative() {
                if let Some(base) = self.toml_path.parent() {
                    config.output_dir = base.join(&config.output_dir);
                }
            }
            Some(config)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub maturin: Option<Maturin>,
    #[serde(rename = "pyo3-stub-gen")]
    pub pyo3_stub_gen: Option<StubGenConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Maturin {
    #[serde(rename = "python-source")]
    pub python_source: Option<String>,
    #[serde(rename = "module-name")]
    pub module_name: Option<String>,
}

/// Configuration options for stub generation from `[tool.pyo3-stub-gen]` in pyproject.toml.
///
/// This struct is marked as `#[non_exhaustive]` to allow adding new configuration
/// options in future versions without breaking backward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StubGenConfig {
    /// Whether to use Python 3.12+ `type` statement syntax for type aliases.
    /// Default is `false` (use pre-3.12 `TypeAlias` syntax).
    #[serde(rename = "use-type-statement", default)]
    pub use_type_statement: bool,
    /// Documentation generation configuration
    #[serde(rename = "doc-gen")]
    pub doc_gen: Option<crate::docgen::DocGenConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_gen_config_true() {
        let toml_str = r#"
            [project]
            name = "test"

            [tool.pyo3-stub-gen]
            use-type-statement = true
        "#;
        let pyproject: PyProject = toml::from_str(toml_str).unwrap();
        assert!(pyproject.stub_gen_config().use_type_statement);
    }

    #[test]
    fn test_stub_gen_config_false() {
        let toml_str = r#"
            [project]
            name = "test"

            [tool.pyo3-stub-gen]
            use-type-statement = false
        "#;
        let pyproject: PyProject = toml::from_str(toml_str).unwrap();
        assert!(!pyproject.stub_gen_config().use_type_statement);
    }

    #[test]
    fn test_stub_gen_config_default() {
        let toml_str = r#"
            [project]
            name = "test"
        "#;
        let pyproject: PyProject = toml::from_str(toml_str).unwrap();
        assert!(!pyproject.stub_gen_config().use_type_statement);
    }

    #[test]
    fn test_stub_gen_config_empty_section() {
        let toml_str = r#"
            [project]
            name = "test"

            [tool.pyo3-stub-gen]
        "#;
        let pyproject: PyProject = toml::from_str(toml_str).unwrap();
        assert!(!pyproject.stub_gen_config().use_type_statement);
    }
}
