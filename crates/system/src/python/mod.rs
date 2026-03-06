//! Python bindings from [PyO3](https://pyo3.rs).

pub mod registry;

// Re-exports
pub use registry::{FactoryRegistry, get_global_pyo3_registry};
