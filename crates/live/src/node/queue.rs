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

use std::{collections::HashMap, fmt::Display};

use nautilus_common::{
    config::{ConfigError, ConfigErrorCollector, ConfigResult, check_valid_value},
    messages::system::{QueueCondition, QueueState},
    runner::SystemChannel,
};
use serde::{Deserialize, Serialize};

use super::metrics::{RunnerMetricsDelta, RunnerMetricsSnapshot};

/// Queue pressure thresholds for one runner channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueueMonitorOverride {
    /// Queue depth that triggers a backlogged state.
    pub queue_depth_trigger: Option<usize>,
    /// Queue depth at or below which a backlogged state clears.
    pub queue_depth_clear: Option<usize>,
    /// Mean dispatch time that triggers a slow state, in nanoseconds.
    pub mean_dispatch_ns_trigger: Option<u64>,
    /// Mean dispatch time at or below which a slow state clears, in nanoseconds.
    pub mean_dispatch_ns_clear: Option<u64>,
}

/// Configuration for runner queue pressure monitoring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueMonitorConfig {
    /// Global queue depth that triggers a backlogged state.
    pub queue_depth_trigger: usize,
    /// Global queue depth at or below which a backlogged state clears.
    pub queue_depth_clear: usize,
    /// Global mean dispatch time that triggers a slow state, in nanoseconds.
    pub mean_dispatch_ns_trigger: u64,
    /// Global mean dispatch time at or below which a slow state clears, in nanoseconds.
    pub mean_dispatch_ns_clear: u64,
    /// Optional threshold overrides keyed by runner channel name.
    #[serde(default)]
    pub overrides: HashMap<String, QueueMonitorOverride>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueueMonitorThresholds {
    pub queue_depth_trigger: usize,
    pub queue_depth_clear: usize,
    pub mean_dispatch_ns_trigger: u64,
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

        let mut override_names = self
            .overrides
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        override_names.sort_unstable();

        for name in override_names {
            let Some(channel) = system_channel_from_name(name) else {
                collector.push(ConfigError::invalid_reference(
                    format!("LiveNodeConfig.queue_monitor.overrides[{name}]"),
                    "system channel",
                    "expected time_events, exec_events, exec_commands, data_events, or data_commands",
                ));
                continue;
            };
            let thresholds = self.thresholds(channel);
            collector.collect(validate_hysteresis(
                format!("LiveNodeConfig.queue_monitor.overrides[{name}].queue_depth"),
                thresholds.queue_depth_trigger,
                thresholds.queue_depth_clear,
            ));
            collector.collect(validate_hysteresis(
                format!("LiveNodeConfig.queue_monitor.overrides[{name}].mean_dispatch_ns"),
                thresholds.mean_dispatch_ns_trigger,
                thresholds.mean_dispatch_ns_clear,
            ));
        }

        collector.into_result()
    }

    pub(crate) fn thresholds(&self, channel: SystemChannel) -> QueueMonitorThresholds {
        let override_config = self.overrides.get(system_channel_name(channel));

        QueueMonitorThresholds {
            queue_depth_trigger: override_config
                .and_then(|config| config.queue_depth_trigger)
                .unwrap_or(self.queue_depth_trigger),
            queue_depth_clear: override_config
                .and_then(|config| config.queue_depth_clear)
                .unwrap_or(self.queue_depth_clear),
            mean_dispatch_ns_trigger: override_config
                .and_then(|config| config.mean_dispatch_ns_trigger)
                .unwrap_or(self.mean_dispatch_ns_trigger),
            mean_dispatch_ns_clear: override_config
                .and_then(|config| config.mean_dispatch_ns_clear)
                .unwrap_or(self.mean_dispatch_ns_clear),
        }
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

const fn system_channel_name(channel: SystemChannel) -> &'static str {
    SYSTEM_CHANNEL_NAMES[system_channel_index(channel)]
}

pub(crate) const SYSTEM_CHANNELS: [SystemChannel; 5] = [
    SystemChannel::TimeEvents,
    SystemChannel::ExecEvents,
    SystemChannel::ExecCommands,
    SystemChannel::DataEvents,
    SystemChannel::DataCommands,
];

const SYSTEM_CHANNEL_NAMES: [&str; SYSTEM_CHANNELS.len()] = [
    "time_events",
    "exec_events",
    "exec_commands",
    "data_events",
    "data_commands",
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

fn system_channel_from_name(name: &str) -> Option<SystemChannel> {
    SYSTEM_CHANNEL_NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| SYSTEM_CHANNELS[index])
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
    thresholds: [QueueMonitorThresholds; SYSTEM_CHANNELS.len()],
    previous_snapshot: RunnerMetricsSnapshot,
    states: [QueueChannelState; SYSTEM_CHANNELS.len()],
}

impl QueueMonitor {
    pub(crate) fn new(
        config: &QueueMonitorConfig,
        previous_snapshot: RunnerMetricsSnapshot,
    ) -> Self {
        Self {
            thresholds: SYSTEM_CHANNELS.map(|channel| config.thresholds(channel)),
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
            let thresholds = self.thresholds[system_channel_index(channel)];
            let queue_depth = channel_queue_depth(snapshot, channel);
            let mean_dispatch_ns = delta.channel_mean_dispatch_ns(channel);
            let dispatched = channel_dispatched(delta, channel);
            let state = &mut self.states[system_channel_index(channel)];

            if let Some(queue_state) = condition_transition(
                &mut state.backlogged,
                &queue_depth,
                &thresholds.queue_depth_trigger,
                &thresholds.queue_depth_clear,
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
                    &thresholds.mean_dispatch_ns_trigger,
                    &thresholds.mean_dispatch_ns_clear,
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
