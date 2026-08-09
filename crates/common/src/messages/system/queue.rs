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

use std::{any::Any, fmt::Display};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::TraderId;

use crate::runner::SystemChannel;

/// Represents a runner queue pressure condition.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueCondition {
    /// The mean dispatch time crossed its configured threshold.
    Slow,
    /// The queue depth crossed its configured threshold.
    Backlogged,
}

/// Represents the state of a runner queue pressure condition.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueState {
    /// The condition crossed its trigger threshold.
    Triggered,
    /// The condition crossed its clear threshold.
    Cleared,
}

/// Represents an event where a runner queue pressure condition has changed.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueStateChanged {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The runner channel associated with the condition.
    pub channel: SystemChannel,
    /// The queue pressure condition.
    pub condition: QueueCondition,
    /// The condition state.
    pub state: QueueState,
    /// The queue depth at the state transition.
    pub queue_depth: usize,
    /// The mean dispatch time per message at the state transition, in nanoseconds.
    pub mean_dispatch_ns: u64,
    /// The event ID.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl QueueStateChanged {
    /// Creates a new [`QueueStateChanged`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        trader_id: TraderId,
        channel: SystemChannel,
        condition: QueueCondition,
        state: QueueState,
        queue_depth: usize,
        mean_dispatch_ns: u64,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            channel,
            condition,
            state,
            queue_depth,
            mean_dispatch_ns,
            event_id,
            ts_event,
            ts_init,
        }
    }

    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Display for QueueStateChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, channel={:?}, condition={:?}, state={:?}, queue_depth={}, mean_dispatch_ns={}, event_id={})",
            stringify!(QueueStateChanged),
            self.trader_id,
            self.channel,
            self.condition,
            self.state,
            self.queue_depth,
            self.mean_dispatch_ns,
            self.event_id,
        )
    }
}

#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "constructor cases vary all fields except the fixed trader ID"
)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        SystemChannel::DataEvents,
        QueueCondition::Slow,
        QueueState::Triggered,
        17,
        23,
        "00000000-0000-4000-8000-000000000001",
        29,
        31
    )]
    #[case(
        SystemChannel::DataCommands,
        QueueCondition::Backlogged,
        QueueState::Cleared,
        37,
        41,
        "00000000-0000-4000-8000-000000000002",
        43,
        47
    )]
    fn test_queue_state_changed_new_assigns_all_fields(
        #[case] channel: SystemChannel,
        #[case] condition: QueueCondition,
        #[case] state: QueueState,
        #[case] queue_depth: usize,
        #[case] mean_dispatch_ns: u64,
        #[case] event_id: &str,
        #[case] ts_event: u64,
        #[case] ts_init: u64,
    ) {
        let trader_id = TraderId::from("TRADER-001");
        let event_id = UUID4::from(event_id);
        let ts_event = UnixNanos::from(ts_event);
        let ts_init = UnixNanos::from(ts_init);

        let event = QueueStateChanged::new(
            trader_id,
            channel,
            condition,
            state,
            queue_depth,
            mean_dispatch_ns,
            event_id,
            ts_event,
            ts_init,
        );

        assert_eq!(event.trader_id, trader_id);
        assert_eq!(event.channel, channel);
        assert_eq!(event.condition, condition);
        assert_eq!(event.state, state);
        assert_eq!(event.queue_depth, queue_depth);
        assert_eq!(event.mean_dispatch_ns, mean_dispatch_ns);
        assert_eq!(event.event_id, event_id);
        assert_eq!(event.ts_event, ts_event);
        assert_eq!(event.ts_init, ts_init);
    }
}
