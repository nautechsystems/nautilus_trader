//! Test actors and strategies for live testing and development.

pub mod data;
pub mod exec;

pub use data::{DataTester, DataTesterConfig};
pub use exec::{ExecTester, ExecTesterConfig};
