//! Workspace shim crate for *NautilusTrader*.
//!
//! This crate does **not** expose any public API of its own – it exists solely so that the root
//! `crates/` directory is a valid Cargo crate.  Having a real compilation unit prevents Cargo
//! from discarding incremental build artefacts for the workspace when only the dependency graph
//! changes, which in turn keeps rebuild times predictable.
//!
//! Because downstream users should never depend on this shim directly, the crate is marked
//! `publish = false` in its `Cargo.toml`.
//!
//! If you are looking for the public entry-points of NautilusTrader, refer to the individual crates
//! under `crates/*` such as `nautilus-core`, `nautilus-model`, `nautilus-backtest`, etc.

// This minimal crate (with a placeholder `src/` and `Cargo.toml`) ensures Cargo treats it as a
// valid compilation unit. Without these placeholders, Cargo might skip or rebuild this directory
// in a way that forces other crates to rebuild. By including a “dummy” crate, we keep the build
// cache fresh and avoid unnecessary full rebuilds.
