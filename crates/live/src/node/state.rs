// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use super::metrics::{RunnerMetrics, RunnerMetricsSnapshot};

const STOP_REQUESTED: u8 = 1 << 7;
const STATE_MASK: u8 = !STOP_REQUESTED;

/// Lifecycle state of the `LiveNode` runner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.live",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.live")
)]
pub enum NodeState {
    #[default]
    Idle = 0,
    Starting = 1,
    Running = 2,
    ShuttingDown = 3,
    Stopped = 4,
}

impl NodeState {
    /// Creates a `NodeState` from its `u8` representation.
    ///
    /// # Panics
    ///
    /// Panics if the value is not a valid `NodeState` discriminant (0-4).
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Idle,
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::ShuttingDown,
            4 => Self::Stopped,
            _ => panic!("Invalid NodeState value"),
        }
    }

    /// Returns the `u8` representation of this state.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns whether the state is `Running`.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Determines which lifecycle responsibilities the node owns while running.
///
/// Both modes run the same event loop. The mode only decides whether the node installs process
/// signal handlers, which a host application must own for itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeRunMode {
    /// The node owns the thread it runs on and installs its own signal handlers.
    #[default]
    Owned,
    /// A host event loop drives the node, and the host owns signal handling and shutdown.
    Hosted,
}

impl NodeRunMode {
    /// Returns whether the node installs process signal handlers in this mode.
    #[must_use]
    pub const fn owns_signals(self) -> bool {
        matches!(self, Self::Owned)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunningTransition {
    Entered,
    StopRequested,
    Invalid(u8),
}

/// A thread-safe handle to control a `LiveNode` from other threads.
///
/// This allows stopping and querying the node's state without requiring the
/// node itself to be Send + Sync.
#[derive(Clone, Debug)]
pub struct LiveNodeHandle {
    control: Arc<AtomicU8>,
    pub(crate) metrics: Arc<RunnerMetrics>,
}

impl Default for LiveNodeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveNodeHandle {
    /// Creates a new handle with default (`Idle`) state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            control: Arc::new(AtomicU8::new(NodeState::Idle.as_u8())),
            metrics: Arc::new(RunnerMetrics::default()),
        }
    }

    pub(crate) fn set_starting(&self) {
        self.set_state(NodeState::Starting);
    }

    pub(crate) fn set_shutting_down(&self) {
        self.set_state(NodeState::ShuttingDown);
    }

    pub(crate) fn set_stopped(&self) {
        self.set_state(NodeState::Stopped);
    }

    pub(super) fn try_set_running(&self) -> RunningTransition {
        match self.control.compare_exchange(
            NodeState::Starting.as_u8(),
            NodeState::Running.as_u8(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => RunningTransition::Entered,
            Err(control) if control == (NodeState::Starting.as_u8() | STOP_REQUESTED) => {
                RunningTransition::StopRequested
            }
            Err(control) => RunningTransition::Invalid(control),
        }
    }

    fn set_state(&self, state: NodeState) {
        let _ = self
            .control
            .try_update(Ordering::AcqRel, Ordering::Acquire, |control| {
                Some((control & STOP_REQUESTED) | state.as_u8())
            });
    }

    /// Returns the current node state.
    #[must_use]
    pub fn state(&self) -> NodeState {
        NodeState::from_u8(self.control.load(Ordering::Acquire) & STATE_MASK)
    }

    /// Returns whether the node should stop.
    #[must_use]
    pub fn should_stop(&self) -> bool {
        self.control.load(Ordering::Acquire) & STOP_REQUESTED != 0
    }

    /// Returns whether the node is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Returns a by-value snapshot of `LiveNode::run` dispatch metrics after startup.
    #[must_use]
    pub fn metrics_snapshot(&self) -> RunnerMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Signals the node to stop.
    pub fn stop(&self) {
        self.control.fetch_or(STOP_REQUESTED, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EngineConnectionStatus {
    Connected,
    TimedOut,
    StopRequested,
    ShutdownRequested,
}

impl EngineConnectionStatus {
    pub(super) const fn abort_reason(self) -> Option<&'static str> {
        match self {
            Self::Connected | Self::TimedOut => None,
            Self::StopRequested => Some("Stop signal received during startup"),
            Self::ShutdownRequested => Some("Shutdown signal received during startup"),
        }
    }
}
