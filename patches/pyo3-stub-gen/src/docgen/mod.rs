//! Documentation generation module for pyo3-stub-gen
//!
//! This module handles generating Sphinx-compatible API reference documentation
//! from the rich type metadata that pyo3-stub-gen possesses.

pub mod builder;
pub mod config;
pub mod default_parser;
pub mod export;
pub mod ir;
pub mod link;
pub mod render;
pub mod types;
pub mod util;

pub use config::DocGenConfig;
pub use ir::DocPackage;
