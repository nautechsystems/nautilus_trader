use std::{
    any::Any,
    fmt::{Debug, Display},
    hash::Hash,
};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::TraderId;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Represents a command to shut down a system and terminate the process.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct ShutdownSystem {
    /// The trader ID associated with the command.
    pub trader_id: TraderId,
    /// The component ID associated with the command.
    pub component_id: Ustr,
    /// The reason for the shutdown command.
    pub reason: Option<String>,
    /// The command ID.
    pub command_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl ShutdownSystem {
    /// Creates a new [`ShutdownSystem`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        component_id: Ustr,
        reason: Option<String>,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            component_id,
            reason,
            command_id,
            ts_init,
        }
    }

    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Display for ShutdownSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, component_id={}, reason={:?}, command_id={})",
            stringify!(ShutdownSystem),
            self.trader_id,
            self.component_id,
            self.reason,
            self.command_id,
        )
    }
}
