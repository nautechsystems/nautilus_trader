use std::{collections::HashMap, time::Duration};

use nautilus_model::{
    enums::{BarAggregation, BarIntervalType},
    identifiers::ClientId,
};

/// Configuration for `DataEngine` instances.
#[derive(Clone, Debug)]
pub struct DataEngineConfig {
    /// If time bar aggregators will build and emit bars with no new market updates.
    pub time_bars_build_with_no_updates: bool,
    /// If time bar aggregators will timestamp `ts_event` on bar close.
    /// If False, then will timestamp on bar open.
    pub time_bars_timestamp_on_close: bool,
    /// If time bar aggregators will skip emitting a bar if the aggregation starts mid-interval.
    pub time_bars_skip_first_non_full_bar: bool,
    /// Determines the type of interval used for time aggregation.
    /// - `LeftOpen`: start time is excluded and end time is included (default).
    /// - `RightOpen`: start time is included and end time is excluded.
    pub time_bars_interval_type: BarIntervalType,
    /// The time delay (microseconds) before building and emitting a bar.
    pub time_bars_build_delay: u64,
    /// A dictionary mapping time bar aggregations to their origin time offsets.
    pub time_bars_origins: HashMap<BarAggregation, Duration>,
    /// If data objects timestamp sequencing will be validated and handled.
    pub validate_data_sequence: bool,
    /// If order book deltas should be buffered until the `F_LAST` flag is set for a delta.
    pub buffer_deltas: bool,
    /// The client IDs declared for external stream processing.
    /// The data engine will not attempt to send data commands to these client IDs.
    pub external_clients: Option<Vec<ClientId>>,
    /// If debug mode is active (will provide extra debug logging).
    pub debug: bool,
}

impl DataEngineConfig {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        time_bars_build_with_no_updates: bool,
        time_bars_timestamp_on_close: bool,
        time_bars_interval_type: BarIntervalType,
        time_bars_skip_first_non_full_bar: bool,
        time_bars_build_delay: u64,
        time_bars_origins: HashMap<BarAggregation, Duration>,
        validate_data_sequence: bool,
        buffer_deltas: bool,
        external_clients: Option<Vec<ClientId>>,
        debug: bool,
    ) -> Self {
        Self {
            time_bars_build_with_no_updates,
            time_bars_timestamp_on_close,
            time_bars_skip_first_non_full_bar,
            time_bars_interval_type,
            time_bars_build_delay,
            time_bars_origins,
            validate_data_sequence,
            buffer_deltas,
            external_clients,
            debug,
        }
    }
}

impl Default for DataEngineConfig {
    fn default() -> Self {
        Self {
            time_bars_build_with_no_updates: true,
            time_bars_timestamp_on_close: true,
            time_bars_interval_type: BarIntervalType::LeftOpen,
            validate_data_sequence: false,
            buffer_deltas: false,
            external_clients: None,
            debug: false,
            time_bars_skip_first_non_full_bar: false,
            time_bars_build_delay: 0,
            time_bars_origins: HashMap::new(),
        }
    }
}
