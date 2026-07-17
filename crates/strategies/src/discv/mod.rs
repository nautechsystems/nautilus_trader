pub mod config;
pub mod strategy;

#[cfg(test)]
mod tests;

pub use config::AddrDiscoveryConfig;
pub use strategy::AddrDiscovery;
