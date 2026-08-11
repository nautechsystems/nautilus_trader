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

use std::fmt::Display;

use nautilus_common::{
    config::{ConfigErrorCollector, ConfigResult, check_valid_value},
    messages::system::{QueueCondition, QueueState},
    runner::SystemChannel,
};
use serde::{Deserialize, Serialize};

use super::metrics::{RunnerMetricsDelta, RunnerMetricsSnapshot};

/// Configuration for runner queue pressure monitoring.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
pub struct QueueMonitorConfig {
    /// Queue depth that triggers a backlogged state.
    pub queue_depth_trigger: usize,
    /// Queue depth at or below which a backlogged state clears.
    pub queue_depth_clear: usize,
    /// Mean dispatch time that triggers a slow state, in nanoseconds.
    pub mean_dispatch_ns_trigger: u64,
    /// Mean dispatch time at or below which a slow state clears, in nanoseconds.
    pub mean_dispatch_ns_clear: u64,
}

impl QueueMonitorConfig {
    pub(crate) fn validate(&self) -> ConfigResult<()> {
        let mut collector = ConfigErrorCollector::new();

        collector.collect(validate_hysteresis(
            "LiveNodeConfig.queue_monitor.queue_depth",
            self.queue_depth_trigger,
            self.queue_depth_clear,
        ));
        collector.collect(validate_hysteresis(
            "LiveNodeConfig.queue_monitor.mean_dispatch_ns",
            self.mean_dispatch_ns_trigger,
            self.mean_dispatch_ns_clear,
        ));

        collector.into_result()
    }
}

fn validate_hysteresis<T>(field: impl Into<String>, trigger: T, clear: T) -> ConfigResult<()>
where
    T: Copy + Display + PartialOrd,
{
    check_valid_value(
        field,
        clear < trigger,
        format!("clear threshold {clear} must be lower than trigger threshold {trigger}"),
    )
}

pub(crate) const SYSTEM_CHANNELS: [SystemChannel; 5] = [
    SystemChannel::TimeEvents,
    SystemChannel::ExecEvents,
    SystemChannel::ExecCommands,
    SystemChannel::DataEvents,
    SystemChannel::DataCommands,
];

pub(crate) const fn system_channel_index(channel: SystemChannel) -> usize {
    match channel {
        SystemChannel::TimeEvents => 0,
        SystemChannel::ExecEvents => 1,
        SystemChannel::ExecCommands => 2,
        SystemChannel::DataEvents => 3,
        SystemChannel::DataCommands => 4,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueueStateTransition {
    pub channel: SystemChannel,
    pub condition: QueueCondition,
    pub state: QueueState,
    pub queue_depth: usize,
    pub mean_dispatch_ns: u64,
}

#[derive(Debug)]
pub(crate) struct QueueMonitor {
    config: QueueMonitorConfig,
    previous_snapshot: RunnerMetricsSnapshot,
    states: [QueueChannelState; SYSTEM_CHANNELS.len()],
}

impl QueueMonitor {
    pub(crate) fn new(
        config: &QueueMonitorConfig,
        previous_snapshot: RunnerMetricsSnapshot,
    ) -> Self {
        Self {
            config: config.clone(),
            previous_snapshot,
            states: [QueueChannelState::default(); SYSTEM_CHANNELS.len()],
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        snapshot: RunnerMetricsSnapshot,
    ) -> Vec<QueueStateTransition> {
        let delta = RunnerMetricsDelta::from_snapshots(self.previous_snapshot, snapshot);
        self.previous_snapshot = snapshot;
        let mut transitions = Vec::new();

        for channel in SYSTEM_CHANNELS {
            let queue_depth = channel_queue_depth(snapshot, channel);
            let mean_dispatch_ns = delta.channel_mean_dispatch_ns(channel);
            let dispatched = channel_dispatched(delta, channel);
            let state = &mut self.states[system_channel_index(channel)];

            if let Some(queue_state) = condition_transition(
                &mut state.backlogged,
                &queue_depth,
                &self.config.queue_depth_trigger,
                &self.config.queue_depth_clear,
            ) {
                transitions.push(QueueStateTransition {
                    channel,
                    condition: QueueCondition::Backlogged,
                    state: queue_state,
                    queue_depth,
                    mean_dispatch_ns,
                });
            }

            // A window without dispatches has no mean sample, so retain the previous slow state.
            if dispatched > 0
                && let Some(queue_state) = condition_transition(
                    &mut state.slow,
                    &mean_dispatch_ns,
                    &self.config.mean_dispatch_ns_trigger,
                    &self.config.mean_dispatch_ns_clear,
                )
            {
                transitions.push(QueueStateTransition {
                    channel,
                    condition: QueueCondition::Slow,
                    state: queue_state,
                    queue_depth,
                    mean_dispatch_ns,
                });
            }
        }

        transitions
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct QueueChannelState {
    slow: bool,
    backlogged: bool,
}

fn condition_transition<T>(
    triggered: &mut bool,
    value: &T,
    trigger_threshold: &T,
    clear_threshold: &T,
) -> Option<QueueState>
where
    T: PartialOrd,
{
    if !*triggered && value >= trigger_threshold {
        *triggered = true;
        Some(QueueState::Triggered)
    } else if *triggered && value <= clear_threshold {
        *triggered = false;
        Some(QueueState::Cleared)
    } else {
        None
    }
}

const fn channel_queue_depth(snapshot: RunnerMetricsSnapshot, channel: SystemChannel) -> usize {
    match channel {
        SystemChannel::TimeEvents => snapshot.time_events.queue_depth,
        SystemChannel::ExecEvents => snapshot.exec_events.queue_depth,
        SystemChannel::ExecCommands => snapshot.exec_commands.queue_depth,
        SystemChannel::DataEvents => snapshot.data_events.queue_depth,
        SystemChannel::DataCommands => snapshot.data_commands.queue_depth,
    }
}

const fn channel_dispatched(delta: RunnerMetricsDelta, channel: SystemChannel) -> u64 {
    match channel {
        SystemChannel::TimeEvents => delta.time_events,
        SystemChannel::ExecEvents => delta.exec_events,
        SystemChannel::ExecCommands => delta.exec_commands,
        SystemChannel::DataEvents => delta.data_events,
        SystemChannel::DataCommands => delta.data_commands,
    }
}
