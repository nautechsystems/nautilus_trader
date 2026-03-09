pub mod client;
pub mod error;
pub mod instruments;
pub mod models;
pub mod parse;
pub mod query;

pub use crate::http::client::TardisHttpClient;

pub const TARDIS_BASE_URL: &str = "https://api.tardis.dev/v1";
