pub mod bar;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod order;
pub mod prices;
pub mod quote;
pub mod trade;

// TODO: https://blog.rust-lang.org/2024/03/30/i128-layout-update.html
// i128 and u128 is now FFI compatible. However, since the clippy lint
// hasn't been removed yet. We'll suppress with #[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]

/// Clones a data instance.
// FFI wrapper for cloning Data instances
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn data_clone(data: &crate::data::Data) -> crate::data::Data {
    data.clone()
}
