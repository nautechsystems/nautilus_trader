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

use std::{
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

use nautilus_common::{
    messages::{DataEvent, ExecutionEvent, data::DataCommand},
    runner::{SystemChannel, TimeEventMessage, TradingCommandMessage},
};

/// Primitive metrics for one `LiveNode::run` dispatch channel after startup.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunnerChannelMetricsSnapshot {
    /// Number of messages dispatched from this channel.
    pub dispatched: u64,
    /// Cumulative nanoseconds spent dispatching from this channel.
    pub dispatch_busy_ns: u64,
    /// Receiver backlog sampled on the runner loop thread.
    pub queue_depth: usize,
    /// Runner-loop elapsed nanoseconds at this channel's last dispatch.
    pub last_dispatch_at_ns: u64,
}

/// Primitive metrics for `LiveNode::run` dispatch and loop work after startup.
///
/// Rates, mean dispatch time, backlog pressure, and utilization are derived by callers from
/// successive snapshots. Values reset each time `LiveNode::run` enters steady state.
/// Residual channel dispatch during shutdown grace is included, but the final post-loop
/// drain is not. Snapshots are lock-free and may not be a consistent cross-field view.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunnerMetricsSnapshot {
    /// Time event channel metrics.
    pub time_events: RunnerChannelMetricsSnapshot,
    /// Execution event channel metrics.
    pub exec_events: RunnerChannelMetricsSnapshot,
    /// Execution command channel metrics.
    pub exec_commands: RunnerChannelMetricsSnapshot,
    /// Data event channel metrics.
    pub data_events: RunnerChannelMetricsSnapshot,
    /// Data command channel metrics.
    pub data_commands: RunnerChannelMetricsSnapshot,
    /// Cumulative nanoseconds spent in the five dispatch branches.
    pub dispatch_busy_ns: u64,
    /// Cumulative nanoseconds spent in maintenance and reconciliation report processing.
    pub maintenance_busy_ns: u64,
    /// Cumulative nanoseconds spent handling external message bus ingress.
    pub external_msgbus_busy_ns: u64,
    /// Monotonic nanoseconds since the steady-state runner loop started.
    pub elapsed_ns: u64,
}

/// Derived deltas between two `LiveNode::run` runner metrics snapshots.
///
/// Values are saturating differences between two [`RunnerMetricsSnapshot`] samples. Queue depths
/// and last-dispatch timestamps remain snapshot-only point-in-time values.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunnerMetricsDelta {
    /// Number of time events dispatched during the sample window.
    pub time_events: u64,
    /// Number of execution events dispatched during the sample window.
    pub exec_events: u64,
    /// Number of execution commands dispatched during the sample window.
    pub exec_commands: u64,
    /// Number of data events dispatched during the sample window.
    pub data_events: u64,
    /// Number of data commands dispatched during the sample window.
    pub data_commands: u64,
    /// Nanoseconds spent dispatching time events during the sample window.
    pub time_events_busy_ns: u64,
    /// Nanoseconds spent dispatching execution events during the sample window.
    pub exec_events_busy_ns: u64,
    /// Nanoseconds spent dispatching execution commands during the sample window.
    pub exec_commands_busy_ns: u64,
    /// Nanoseconds spent dispatching data events during the sample window.
    pub data_events_busy_ns: u64,
    /// Nanoseconds spent dispatching data commands during the sample window.
    pub data_commands_busy_ns: u64,
    /// Nanoseconds spent in the five dispatch branches during the sample window.
    pub dispatch_busy_ns: u64,
    /// Nanoseconds spent in maintenance and reconciliation processing during the sample window.
    pub maintenance_busy_ns: u64,
    /// Nanoseconds spent handling external message bus ingress during the sample window.
    pub external_msgbus_busy_ns: u64,
    /// Monotonic nanoseconds elapsed during the sample window.
    pub elapsed_ns: u64,
}

impl RunnerMetricsDelta {
    /// Returns the saturating delta between two runner metrics snapshots.
    #[must_use]
    pub fn from_snapshots(before: RunnerMetricsSnapshot, after: RunnerMetricsSnapshot) -> Self {
        let (time_events, time_events_busy_ns) =
            channel_dispatch_delta(before.time_events, after.time_events);
        let (exec_events, exec_events_busy_ns) =
            channel_dispatch_delta(before.exec_events, after.exec_events);
        let (exec_commands, exec_commands_busy_ns) =
            channel_dispatch_delta(before.exec_commands, after.exec_commands);
        let (data_events, data_events_busy_ns) =
            channel_dispatch_delta(before.data_events, after.data_events);
        let (data_commands, data_commands_busy_ns) =
            channel_dispatch_delta(before.data_commands, after.data_commands);

        Self {
            time_events,
            exec_events,
            exec_commands,
            data_events,
            data_commands,
            time_events_busy_ns,
            exec_events_busy_ns,
            exec_commands_busy_ns,
            data_events_busy_ns,
            data_commands_busy_ns,
            dispatch_busy_ns: after
                .dispatch_busy_ns
                .saturating_sub(before.dispatch_busy_ns),
            maintenance_busy_ns: after
                .maintenance_busy_ns
                .saturating_sub(before.maintenance_busy_ns),
            external_msgbus_busy_ns: after
                .external_msgbus_busy_ns
                .saturating_sub(before.external_msgbus_busy_ns),
            elapsed_ns: after.elapsed_ns.saturating_sub(before.elapsed_ns),
        }
    }

    /// Returns the total messages dispatched across all runner channels.
    #[must_use]
    pub const fn total_dispatched(&self) -> u64 {
        self.time_events
            .saturating_add(self.exec_events)
            .saturating_add(self.exec_commands)
            .saturating_add(self.data_events)
            .saturating_add(self.data_commands)
    }

    /// Returns dispatch busy time divided by elapsed time for the sample window.
    ///
    /// Returns `0.0` when the elapsed window is zero.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "sample-window utilization is an approximate ratio"
    )]
    pub fn dispatch_utilization(&self) -> f64 {
        if self.elapsed_ns == 0 {
            0.0
        } else {
            self.dispatch_busy_ns as f64 / self.elapsed_ns as f64
        }
    }

    /// Returns total timed runner-loop work divided by elapsed time for the sample window.
    ///
    /// Total work includes dispatch, maintenance, reconciliation, and external message bus ingress
    /// handling. Returns `0.0` when the elapsed window is zero.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "sample-window utilization is an approximate ratio"
    )]
    pub fn loop_utilization(&self) -> f64 {
        if self.elapsed_ns == 0 {
            0.0
        } else {
            self.total_busy_ns() as f64 / self.elapsed_ns as f64
        }
    }

    /// Returns mean dispatch time in nanoseconds for the sample window.
    ///
    /// Returns zero when no dispatches were recorded.
    #[must_use]
    pub fn mean_dispatch_ns(&self) -> u64 {
        self.dispatch_busy_ns
            .checked_div(self.total_dispatched())
            .unwrap_or(0)
    }

    /// Returns mean dispatch time in nanoseconds for `channel` during the sample window.
    ///
    /// Returns zero when the channel dispatched no messages.
    #[must_use]
    pub fn channel_mean_dispatch_ns(&self, channel: SystemChannel) -> u64 {
        let (dispatched, dispatch_busy_ns) = match channel {
            SystemChannel::TimeEvents => (self.time_events, self.time_events_busy_ns),
            SystemChannel::ExecEvents => (self.exec_events, self.exec_events_busy_ns),
            SystemChannel::ExecCommands => (self.exec_commands, self.exec_commands_busy_ns),
            SystemChannel::DataEvents => (self.data_events, self.data_events_busy_ns),
            SystemChannel::DataCommands => (self.data_commands, self.data_commands_busy_ns),
        };

        dispatch_busy_ns.checked_div(dispatched).unwrap_or(0)
    }

    /// Returns the total nanoseconds spent in timed runner-loop work.
    #[must_use]
    pub const fn total_busy_ns(&self) -> u64 {
        self.dispatch_busy_ns
            .saturating_add(self.maintenance_busy_ns)
            .saturating_add(self.external_msgbus_busy_ns)
    }
}

fn channel_dispatch_delta(
    before: RunnerChannelMetricsSnapshot,
    after: RunnerChannelMetricsSnapshot,
) -> (u64, u64) {
    (
        after.dispatched.saturating_sub(before.dispatched),
        after
            .dispatch_busy_ns
            .saturating_sub(before.dispatch_busy_ns),
    )
}

#[derive(Debug, Default)]
pub(crate) struct RunnerMetrics {
    time_events: RunnerChannelMetrics,
    exec_events: RunnerChannelMetrics,
    exec_commands: RunnerChannelMetrics,
    data_events: RunnerChannelMetrics,
    data_commands: RunnerChannelMetrics,
    maintenance_busy_ns: AtomicU64,
    external_msgbus_busy_ns: AtomicU64,
    elapsed_ns: AtomicU64,
}

impl RunnerMetrics {
    pub(crate) fn reset(&self) {
        self.time_events.reset();
        self.exec_events.reset();
        self.exec_commands.reset();
        self.data_events.reset();
        self.data_commands.reset();
        self.maintenance_busy_ns.store(0, Ordering::Relaxed);
        self.external_msgbus_busy_ns.store(0, Ordering::Relaxed);
        self.elapsed_ns.store(0, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> RunnerMetricsSnapshot {
        let time_events = self.time_events.snapshot();
        let exec_events = self.exec_events.snapshot();
        let exec_commands = self.exec_commands.snapshot();
        let data_events = self.data_events.snapshot();
        let data_commands = self.data_commands.snapshot();

        RunnerMetricsSnapshot {
            time_events,
            exec_events,
            exec_commands,
            data_events,
            data_commands,
            dispatch_busy_ns: time_events
                .dispatch_busy_ns
                .saturating_add(exec_events.dispatch_busy_ns)
                .saturating_add(exec_commands.dispatch_busy_ns)
                .saturating_add(data_events.dispatch_busy_ns)
                .saturating_add(data_commands.dispatch_busy_ns),
            maintenance_busy_ns: self.maintenance_busy_ns.load(Ordering::Relaxed),
            external_msgbus_busy_ns: self.external_msgbus_busy_ns.load(Ordering::Relaxed),
            elapsed_ns: self.elapsed_ns.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn record_dispatch(
        &self,
        channel: SystemChannel,
        dispatch_elapsed: Duration,
        elapsed_since_start: Duration,
    ) {
        let elapsed_ns = duration_ns(elapsed_since_start);
        self.channel(channel)
            .record_dispatch(duration_ns(dispatch_elapsed), elapsed_ns);
        self.elapsed_ns.store(elapsed_ns, Ordering::Relaxed);
    }

    pub(crate) fn record_maintenance(&self, work_elapsed: Duration, elapsed_since_start: Duration) {
        self.record_loop_work(&self.maintenance_busy_ns, work_elapsed, elapsed_since_start);
    }

    pub(crate) fn record_external_msgbus(
        &self,
        work_elapsed: Duration,
        elapsed_since_start: Duration,
    ) {
        self.record_loop_work(
            &self.external_msgbus_busy_ns,
            work_elapsed,
            elapsed_since_start,
        );
    }

    pub(crate) fn publish_queue_depths(
        &self,
        depths: RunnerChannelQueueDepths,
        elapsed_since_start: Duration,
    ) {
        self.time_events.set_queue_depth(depths.time_events);
        self.exec_events.set_queue_depth(depths.exec_events);
        self.exec_commands.set_queue_depth(depths.exec_commands);
        self.data_events.set_queue_depth(depths.data_events);
        self.data_commands.set_queue_depth(depths.data_commands);
        self.elapsed_ns
            .store(duration_ns(elapsed_since_start), Ordering::Relaxed);
    }

    fn channel(&self, channel: SystemChannel) -> &RunnerChannelMetrics {
        match channel {
            SystemChannel::TimeEvents => &self.time_events,
            SystemChannel::ExecEvents => &self.exec_events,
            SystemChannel::ExecCommands => &self.exec_commands,
            SystemChannel::DataEvents => &self.data_events,
            SystemChannel::DataCommands => &self.data_commands,
        }
    }

    fn record_loop_work(
        &self,
        busy_ns: &AtomicU64,
        work_elapsed: Duration,
        elapsed_since_start: Duration,
    ) {
        saturating_fetch_add(busy_ns, duration_ns(work_elapsed));
        self.elapsed_ns
            .store(duration_ns(elapsed_since_start), Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunnerChannelQueueDepths {
    time_events: usize,
    exec_events: usize,
    exec_commands: usize,
    data_events: usize,
    data_commands: usize,
}

impl RunnerChannelQueueDepths {
    pub(crate) fn from_receivers(
        time_events: &tokio::sync::mpsc::UnboundedReceiver<TimeEventMessage>,
        exec_events: &tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        exec_commands: &tokio::sync::mpsc::UnboundedReceiver<TradingCommandMessage>,
        data_events: &tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        data_commands: &tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
    ) -> Self {
        Self {
            time_events: time_events.len(),
            exec_events: exec_events.len(),
            exec_commands: exec_commands.len(),
            data_events: data_events.len(),
            data_commands: data_commands.len(),
        }
    }
}

#[derive(Debug, Default)]
struct RunnerChannelMetrics {
    dispatched: AtomicU64,
    dispatch_busy_ns: AtomicU64,
    queue_depth: AtomicUsize,
    last_dispatch_at_ns: AtomicU64,
}

impl RunnerChannelMetrics {
    fn reset(&self) {
        self.dispatched.store(0, Ordering::Relaxed);
        self.dispatch_busy_ns.store(0, Ordering::Relaxed);
        self.queue_depth.store(0, Ordering::Relaxed);
        self.last_dispatch_at_ns.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> RunnerChannelMetricsSnapshot {
        RunnerChannelMetricsSnapshot {
            dispatched: self.dispatched.load(Ordering::Relaxed),
            dispatch_busy_ns: self.dispatch_busy_ns.load(Ordering::Relaxed),
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
            last_dispatch_at_ns: self.last_dispatch_at_ns.load(Ordering::Relaxed),
        }
    }

    fn record_dispatch(&self, dispatch_busy_ns: u64, last_dispatch_at_ns: u64) {
        self.dispatched.fetch_add(1, Ordering::Relaxed);
        saturating_fetch_add(&self.dispatch_busy_ns, dispatch_busy_ns);
        self.last_dispatch_at_ns
            .store(last_dispatch_at_ns, Ordering::Relaxed);
    }

    fn set_queue_depth(&self, queue_depth: usize) {
        self.queue_depth.store(queue_depth, Ordering::Relaxed);
    }
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn saturating_fetch_add(atomic: &AtomicU64, value: u64) {
    atomic
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_add(value))
        })
        .expect("try_update closure returns Some");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nautilus_common::{
        messages::{
            data::{SubscribeCommand, subscribe::SubscribeInstruments},
            execution::{QueryAccount, TradingCommand},
            system::{QueueCondition, QueueState},
        },
        msgbus::MessagingSwitchboard,
        timer::{TimeEvent, TimeEventCallback},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::AccountType,
        events::account::state::AccountState,
        identifiers::{AccountId, TraderId, Venue},
        instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::{
        super::queue::{QueueMonitor, QueueMonitorConfig, QueueStateTransition},
        *,
    };

    #[rstest]
    fn test_runner_metrics_default_snapshot_is_zero() {
        let metrics = RunnerMetrics::default();

        assert_eq!(metrics.snapshot(), RunnerMetricsSnapshot::default());
    }

    #[rstest]
    fn test_runner_metrics_delta_saturates_when_after_is_lower_than_before() {
        let before = runner_snapshot([10, 9, 8, 7, 6], [20, 20, 20, 20, 20], 90, 80, 70);
        let after = runner_snapshot([5, 4, 3, 2, 1], [10, 10, 10, 10, 10], 40, 30, 20);

        let delta = RunnerMetricsDelta::from_snapshots(before, after);

        assert_eq!(delta, RunnerMetricsDelta::default());
    }

    #[rstest]
    fn test_runner_metrics_delta_zero_elapsed_window_returns_zero_utilization() {
        let delta = RunnerMetricsDelta::from_snapshots(
            RunnerMetricsSnapshot::default(),
            runner_snapshot([1, 0, 0, 0, 0], [10, 0, 0, 0, 0], 20, 30, 0),
        );

        assert!(delta.dispatch_utilization().abs() < f64::EPSILON);
        assert!(delta.loop_utilization().abs() < f64::EPSILON);
    }

    #[rstest]
    fn test_runner_metrics_delta_zero_dispatched_returns_zero_mean_dispatch_time() {
        let delta = RunnerMetricsDelta::from_snapshots(
            RunnerMetricsSnapshot::default(),
            runner_snapshot([0, 0, 0, 0, 0], [100, 0, 0, 0, 0], 0, 0, 100),
        );

        assert_eq!(delta.mean_dispatch_ns(), 0);
        assert_eq!(delta.channel_mean_dispatch_ns(SystemChannel::TimeEvents), 0);
    }

    #[rstest]
    fn test_runner_metrics_delta_total_dispatched_sums_all_channels() {
        let delta = RunnerMetricsDelta::from_snapshots(
            RunnerMetricsSnapshot::default(),
            runner_snapshot([1, 2, 3, 4, 5], [0, 0, 0, 0, 0], 0, 0, 100),
        );

        assert_eq!(delta.total_dispatched(), 15);
    }

    #[rstest]
    fn test_runner_metrics_delta_derived_metrics_use_sample_window_values() {
        let before = runner_snapshot([1, 2, 0, 0, 0], [40, 20, 30, 10, 0], 10, 5, 200);
        let after = runner_snapshot([4, 3, 2, 1, 0], [70, 40, 40, 10, 0], 30, 15, 300);

        let delta = RunnerMetricsDelta::from_snapshots(before, after);

        assert_eq!(delta.total_dispatched(), 7);
        assert_eq!(delta.dispatch_busy_ns, 60);
        assert_eq!(delta.total_busy_ns(), 90);
        assert_eq!(delta.mean_dispatch_ns(), 8);
        assert!((delta.dispatch_utilization() - 0.6).abs() < f64::EPSILON);
        assert!((delta.loop_utilization() - 0.9).abs() < f64::EPSILON);
    }

    #[rstest]
    #[case(SystemChannel::TimeEvents, 10)]
    #[case(SystemChannel::ExecEvents, 20)]
    #[case(SystemChannel::ExecCommands, 5)]
    #[case(SystemChannel::DataEvents, 4)]
    #[case(SystemChannel::DataCommands, 0)]
    fn test_runner_metrics_delta_channel_mean_dispatch_ns_divides_selected_channel(
        #[case] channel: SystemChannel,
        #[case] expected_mean_ns: u64,
    ) {
        let before = runner_snapshot([1, 2, 3, 4, 5], [10, 20, 30, 40, 50], 0, 0, 100);
        let after = runner_snapshot([4, 3, 5, 9, 5], [40, 40, 40, 60, 90], 0, 0, 200);

        let delta = RunnerMetricsDelta::from_snapshots(before, after);

        assert_eq!(delta.channel_mean_dispatch_ns(channel), expected_mean_ns);
    }

    #[rstest]
    fn test_runner_metrics_delta_channel_busy_ns_sums_to_dispatch_busy_ns() {
        let before = runner_snapshot([1, 2, 3, 4, 5], [10, 20, 30, 40, 50], 0, 0, 100);
        let after = runner_snapshot([4, 3, 5, 9, 5], [40, 40, 40, 60, 90], 0, 0, 200);

        let delta = RunnerMetricsDelta::from_snapshots(before, after);

        assert_eq!(delta.time_events_busy_ns, 30);
        assert_eq!(delta.exec_events_busy_ns, 20);
        assert_eq!(delta.exec_commands_busy_ns, 10);
        assert_eq!(delta.data_events_busy_ns, 20);
        assert_eq!(delta.data_commands_busy_ns, 40);
        assert_eq!(delta.dispatch_busy_ns, 120);
    }

    #[rstest]
    fn test_queue_monitor_uses_successive_snapshot_delta_and_crossing_values() {
        let previous = with_queue_depths(
            runner_snapshot([10, 0, 0, 0, 0], [1_000, 0, 0, 0, 0], 0, 0, 100),
            [999, 0, 0, 0, 0],
        );
        let mut monitor = QueueMonitor::new(&queue_monitor_config(), previous);
        let snapshot = with_queue_depths(
            runner_snapshot([12, 0, 0, 0, 0], [1_300, 0, 0, 0, 0], 0, 0, 200),
            [10, 0, 0, 0, 0],
        );

        let transitions = monitor.evaluate(snapshot);

        assert_eq!(
            transitions,
            vec![
                QueueStateTransition {
                    channel: SystemChannel::TimeEvents,
                    condition: QueueCondition::Backlogged,
                    state: QueueState::Triggered,
                    queue_depth: 10,
                    mean_dispatch_ns: 150,
                },
                QueueStateTransition {
                    channel: SystemChannel::TimeEvents,
                    condition: QueueCondition::Slow,
                    state: QueueState::Triggered,
                    queue_depth: 10,
                    mean_dispatch_ns: 150,
                },
            ]
        );
    }

    #[rstest]
    fn test_queue_monitor_hysteresis_does_not_flap_between_thresholds() {
        let mut monitor =
            QueueMonitor::new(&queue_monitor_config(), RunnerMetricsSnapshot::default());
        let triggered = with_queue_depths(
            runner_snapshot([1, 0, 0, 0, 0], [100, 0, 0, 0, 0], 0, 0, 100),
            [10, 0, 0, 0, 0],
        );
        let between = with_queue_depths(
            runner_snapshot([2, 0, 0, 0, 0], [175, 0, 0, 0, 0], 0, 0, 200),
            [7, 0, 0, 0, 0],
        );
        let cleared = with_queue_depths(
            runner_snapshot([3, 0, 0, 0, 0], [225, 0, 0, 0, 0], 0, 0, 300),
            [5, 0, 0, 0, 0],
        );

        assert_eq!(monitor.evaluate(triggered).len(), 2);
        assert!(monitor.evaluate(between).is_empty());
        assert_eq!(
            monitor.evaluate(cleared),
            vec![
                QueueStateTransition {
                    channel: SystemChannel::TimeEvents,
                    condition: QueueCondition::Backlogged,
                    state: QueueState::Cleared,
                    queue_depth: 5,
                    mean_dispatch_ns: 50,
                },
                QueueStateTransition {
                    channel: SystemChannel::TimeEvents,
                    condition: QueueCondition::Slow,
                    state: QueueState::Cleared,
                    queue_depth: 5,
                    mean_dispatch_ns: 50,
                },
            ]
        );
    }

    #[rstest]
    fn test_queue_monitor_holds_slow_state_without_dispatch_sample() {
        let mut monitor =
            QueueMonitor::new(&queue_monitor_config(), RunnerMetricsSnapshot::default());
        let triggered = runner_snapshot([1, 0, 0, 0, 0], [100, 0, 0, 0, 0], 0, 0, 100);
        let idle = runner_snapshot([1, 0, 0, 0, 0], [100, 0, 0, 0, 0], 0, 0, 200);
        let cleared = runner_snapshot([2, 0, 0, 0, 0], [150, 0, 0, 0, 0], 0, 0, 300);

        assert_eq!(
            monitor.evaluate(triggered),
            vec![QueueStateTransition {
                channel: SystemChannel::TimeEvents,
                condition: QueueCondition::Slow,
                state: QueueState::Triggered,
                queue_depth: 0,
                mean_dispatch_ns: 100,
            }]
        );
        assert!(monitor.evaluate(idle).is_empty());
        assert_eq!(
            monitor.evaluate(cleared),
            vec![QueueStateTransition {
                channel: SystemChannel::TimeEvents,
                condition: QueueCondition::Slow,
                state: QueueState::Cleared,
                queue_depth: 0,
                mean_dispatch_ns: 50,
            }]
        );
    }

    #[rstest]
    fn test_queue_monitor_conditions_trigger_and_clear_independently() {
        let mut monitor =
            QueueMonitor::new(&queue_monitor_config(), RunnerMetricsSnapshot::default());
        let triggered = with_queue_depths(
            runner_snapshot([1, 0, 0, 0, 0], [100, 0, 0, 0, 0], 0, 0, 100),
            [10, 0, 0, 0, 0],
        );
        let slow_cleared = with_queue_depths(
            runner_snapshot([2, 0, 0, 0, 0], [150, 0, 0, 0, 0], 0, 0, 200),
            [7, 0, 0, 0, 0],
        );
        let backlog_cleared = with_queue_depths(
            runner_snapshot([3, 0, 0, 0, 0], [225, 0, 0, 0, 0], 0, 0, 300),
            [5, 0, 0, 0, 0],
        );

        assert_eq!(monitor.evaluate(triggered).len(), 2);
        assert_eq!(
            monitor.evaluate(slow_cleared),
            vec![QueueStateTransition {
                channel: SystemChannel::TimeEvents,
                condition: QueueCondition::Slow,
                state: QueueState::Cleared,
                queue_depth: 7,
                mean_dispatch_ns: 50,
            }]
        );
        assert_eq!(
            monitor.evaluate(backlog_cleared),
            vec![QueueStateTransition {
                channel: SystemChannel::TimeEvents,
                condition: QueueCondition::Backlogged,
                state: QueueState::Cleared,
                queue_depth: 5,
                mean_dispatch_ns: 75,
            }]
        );
    }

    #[rstest]
    fn test_queue_monitor_keeps_channel_state_isolated() {
        let mut monitor =
            QueueMonitor::new(&queue_monitor_config(), RunnerMetricsSnapshot::default());
        let first = with_queue_depths(
            runner_snapshot([0, 0, 0, 1, 0], [0, 0, 0, 100, 0], 0, 0, 100),
            [0, 0, 0, 10, 0],
        );
        let second = with_queue_depths(
            runner_snapshot([0, 1, 0, 2, 0], [0, 100, 0, 175, 0], 0, 0, 200),
            [0, 10, 0, 7, 0],
        );

        assert_eq!(
            monitor
                .evaluate(first)
                .iter()
                .map(|transition| transition.channel)
                .collect::<Vec<_>>(),
            vec![SystemChannel::DataEvents, SystemChannel::DataEvents]
        );
        assert_eq!(
            monitor.evaluate(second),
            vec![
                QueueStateTransition {
                    channel: SystemChannel::ExecEvents,
                    condition: QueueCondition::Backlogged,
                    state: QueueState::Triggered,
                    queue_depth: 10,
                    mean_dispatch_ns: 100,
                },
                QueueStateTransition {
                    channel: SystemChannel::ExecEvents,
                    condition: QueueCondition::Slow,
                    state: QueueState::Triggered,
                    queue_depth: 10,
                    mean_dispatch_ns: 100,
                },
            ]
        );
    }

    #[rstest]
    fn test_runner_metrics_snapshot_reflects_dispatch_updates() {
        let metrics = RunnerMetrics::default();

        metrics.record_dispatch(
            SystemChannel::ExecCommands,
            Duration::from_nanos(10),
            Duration::from_nanos(50),
        );
        metrics.record_dispatch(
            SystemChannel::DataEvents,
            Duration::from_nanos(7),
            Duration::from_nanos(90),
        );

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.exec_commands.dispatched, 1);
        assert_eq!(snapshot.exec_commands.dispatch_busy_ns, 10);
        assert_eq!(snapshot.exec_commands.last_dispatch_at_ns, 50);
        assert_eq!(snapshot.data_events.dispatched, 1);
        assert_eq!(snapshot.data_events.dispatch_busy_ns, 7);
        assert_eq!(snapshot.data_events.last_dispatch_at_ns, 90);
        assert_eq!(snapshot.dispatch_busy_ns, 17);
        assert_eq!(snapshot.maintenance_busy_ns, 0);
        assert_eq!(snapshot.external_msgbus_busy_ns, 0);
        assert_eq!(snapshot.elapsed_ns, 90);
    }

    #[rstest]
    #[case(SystemChannel::TimeEvents, [1, 0, 0, 0, 0], [10, 0, 0, 0, 0], [50, 0, 0, 0, 0])]
    #[case(SystemChannel::ExecEvents, [0, 1, 0, 0, 0], [0, 10, 0, 0, 0], [0, 50, 0, 0, 0])]
    #[case(SystemChannel::ExecCommands, [0, 0, 1, 0, 0], [0, 0, 10, 0, 0], [0, 0, 50, 0, 0])]
    #[case(SystemChannel::DataEvents, [0, 0, 0, 1, 0], [0, 0, 0, 10, 0], [0, 0, 0, 50, 0])]
    #[case(SystemChannel::DataCommands, [0, 0, 0, 0, 1], [0, 0, 0, 0, 10], [0, 0, 0, 0, 50])]
    fn test_runner_metrics_record_dispatch_updates_selected_channel(
        #[case] channel: SystemChannel,
        #[case] expected_dispatched: [u64; 5],
        #[case] expected_dispatch_busy_ns: [u64; 5],
        #[case] expected_last_dispatch: [u64; 5],
    ) {
        let metrics = RunnerMetrics::default();

        metrics.record_dispatch(channel, Duration::from_nanos(10), Duration::from_nanos(50));
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot_dispatch_counts(snapshot), expected_dispatched);
        assert_eq!(
            snapshot_dispatch_busy_ns(snapshot),
            expected_dispatch_busy_ns
        );
        assert_eq!(
            snapshot_last_dispatch_at_ns(snapshot),
            expected_last_dispatch
        );
        assert_eq!(snapshot.dispatch_busy_ns, 10);
        assert_eq!(snapshot.elapsed_ns, 50);
    }

    #[rstest]
    fn test_runner_metrics_snapshot_reflects_loop_work_updates() {
        let metrics = RunnerMetrics::default();

        metrics.record_maintenance(Duration::from_nanos(10), Duration::from_nanos(50));
        metrics.record_external_msgbus(Duration::from_nanos(7), Duration::from_nanos(90));

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.dispatch_busy_ns, 0);
        assert_eq!(snapshot.maintenance_busy_ns, 10);
        assert_eq!(snapshot.external_msgbus_busy_ns, 7);
        assert_eq!(snapshot.elapsed_ns, 90);
    }

    #[rstest]
    fn test_runner_metrics_reset_clears_populated_snapshot() {
        let metrics = RunnerMetrics::default();

        metrics.record_dispatch(
            SystemChannel::TimeEvents,
            Duration::from_nanos(10),
            Duration::from_nanos(30),
        );
        metrics.record_maintenance(Duration::from_nanos(5), Duration::from_nanos(40));
        metrics.record_external_msgbus(Duration::from_nanos(7), Duration::from_nanos(45));
        metrics.publish_queue_depths(
            RunnerChannelQueueDepths {
                time_events: 1,
                exec_events: 2,
                exec_commands: 3,
                data_events: 4,
                data_commands: 5,
            },
            Duration::from_nanos(50),
        );
        metrics.reset();

        assert_eq!(metrics.snapshot(), RunnerMetricsSnapshot::default());
    }

    #[rstest]
    fn test_runner_metrics_queue_depths_use_receiver_lengths() {
        let (time_tx, time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
        let (data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let metrics = RunnerMetrics::default();

        time_tx.send(stub_time_event_handler()).unwrap();
        for _ in 0..2 {
            exec_evt_tx.send(stub_exec_event()).unwrap();
        }

        for _ in 0..3 {
            exec_cmd_tx
                .send(TradingCommandMessage::new(
                    MessagingSwitchboard::exec_engine_execute(),
                    stub_trading_command(),
                ))
                .unwrap();
        }

        for _ in 0..4 {
            data_evt_tx.send(stub_data_event()).unwrap();
        }

        for _ in 0..5 {
            data_cmd_tx.send(stub_data_command()).unwrap();
        }

        metrics.publish_queue_depths(
            RunnerChannelQueueDepths::from_receivers(
                &time_rx,
                &exec_evt_rx,
                &exec_cmd_rx,
                &data_evt_rx,
                &data_cmd_rx,
            ),
            Duration::from_nanos(25),
        );
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.time_events.queue_depth, 1);
        assert_eq!(snapshot.exec_events.queue_depth, 2);
        assert_eq!(snapshot.exec_commands.queue_depth, 3);
        assert_eq!(snapshot.data_events.queue_depth, 4);
        assert_eq!(snapshot.data_commands.queue_depth, 5);
        assert_eq!(snapshot.elapsed_ns, 25);
    }

    fn runner_snapshot(
        dispatched: [u64; 5],
        dispatch_busy_ns: [u64; 5],
        maintenance_busy_ns: u64,
        external_msgbus_busy_ns: u64,
        elapsed_ns: u64,
    ) -> RunnerMetricsSnapshot {
        let total_dispatch_busy_ns = dispatch_busy_ns.into_iter().fold(0, u64::saturating_add);
        let [
            time_events,
            exec_events,
            exec_commands,
            data_events,
            data_commands,
        ] = dispatched;
        let [
            time_events_busy_ns,
            exec_events_busy_ns,
            exec_commands_busy_ns,
            data_events_busy_ns,
            data_commands_busy_ns,
        ] = dispatch_busy_ns;

        RunnerMetricsSnapshot {
            time_events: channel_snapshot(time_events, time_events_busy_ns),
            exec_events: channel_snapshot(exec_events, exec_events_busy_ns),
            exec_commands: channel_snapshot(exec_commands, exec_commands_busy_ns),
            data_events: channel_snapshot(data_events, data_events_busy_ns),
            data_commands: channel_snapshot(data_commands, data_commands_busy_ns),
            dispatch_busy_ns: total_dispatch_busy_ns,
            maintenance_busy_ns,
            external_msgbus_busy_ns,
            elapsed_ns,
        }
    }

    fn with_queue_depths(
        mut snapshot: RunnerMetricsSnapshot,
        depths: [usize; 5],
    ) -> RunnerMetricsSnapshot {
        let [
            time_events,
            exec_events,
            exec_commands,
            data_events,
            data_commands,
        ] = depths;
        snapshot.time_events.queue_depth = time_events;
        snapshot.exec_events.queue_depth = exec_events;
        snapshot.exec_commands.queue_depth = exec_commands;
        snapshot.data_events.queue_depth = data_events;
        snapshot.data_commands.queue_depth = data_commands;
        snapshot
    }

    fn queue_monitor_config() -> QueueMonitorConfig {
        QueueMonitorConfig {
            queue_depth_trigger: 10,
            queue_depth_clear: 5,
            mean_dispatch_ns_trigger: 100,
            mean_dispatch_ns_clear: 50,
        }
    }

    fn channel_snapshot(dispatched: u64, dispatch_busy_ns: u64) -> RunnerChannelMetricsSnapshot {
        RunnerChannelMetricsSnapshot {
            dispatched,
            dispatch_busy_ns,
            ..Default::default()
        }
    }

    fn snapshot_dispatch_counts(snapshot: RunnerMetricsSnapshot) -> [u64; 5] {
        [
            snapshot.time_events.dispatched,
            snapshot.exec_events.dispatched,
            snapshot.exec_commands.dispatched,
            snapshot.data_events.dispatched,
            snapshot.data_commands.dispatched,
        ]
    }

    fn snapshot_dispatch_busy_ns(snapshot: RunnerMetricsSnapshot) -> [u64; 5] {
        [
            snapshot.time_events.dispatch_busy_ns,
            snapshot.exec_events.dispatch_busy_ns,
            snapshot.exec_commands.dispatch_busy_ns,
            snapshot.data_events.dispatch_busy_ns,
            snapshot.data_commands.dispatch_busy_ns,
        ]
    }

    fn snapshot_last_dispatch_at_ns(snapshot: RunnerMetricsSnapshot) -> [u64; 5] {
        [
            snapshot.time_events.last_dispatch_at_ns,
            snapshot.exec_events.last_dispatch_at_ns,
            snapshot.exec_commands.last_dispatch_at_ns,
            snapshot.data_events.last_dispatch_at_ns,
            snapshot.data_commands.last_dispatch_at_ns,
        ]
    }

    fn stub_time_event_handler() -> TimeEventMessage {
        TimeEventMessage::new(
            TimeEvent::new(
                Ustr::from("test-timer"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
            TimeEventCallback::from(|_| {}),
        )
    }

    fn stub_exec_event() -> ExecutionEvent {
        ExecutionEvent::Account(AccountState::new(
            AccountId::from("TEST-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        ))
    }

    fn stub_trading_command() -> TradingCommand {
        TradingCommand::QueryAccount(QueryAccount::new(
            TraderId::from("TESTER-001"),
            None,
            AccountId::from("TEST-001"),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
    }

    fn stub_data_event() -> DataEvent {
        DataEvent::Instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))
    }

    fn stub_data_command() -> DataCommand {
        DataCommand::Subscribe(SubscribeCommand::Instruments(SubscribeInstruments::new(
            None,
            Venue::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )))
    }
}
