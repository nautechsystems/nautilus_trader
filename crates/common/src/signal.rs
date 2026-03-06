//! A user signal type.

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Represents a generic signal.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common", from_py_object)
)]
pub struct Signal {
    pub name: Ustr,
    pub value: String,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl Signal {
    /// Creates a new [`Signal`] instance.
    #[must_use]
    pub const fn new(name: Ustr, value: String, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        Self {
            name,
            value,
            ts_event,
            ts_init,
        }
    }
}
