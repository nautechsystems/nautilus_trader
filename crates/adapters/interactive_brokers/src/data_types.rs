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

//! Interactive Brokers subscription diagnostics delivered as custom data.

use nautilus_core::UnixNanos;
use nautilus_model::{
    custom_data,
    identifiers::{ClientId, InstrumentId},
};

/// A subscription receives no market data within its configured interval.
///
/// This is an inactivity signal, not evidence that the connection or market is closed.
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.interactive_brokers")]
pub struct InteractiveBrokersSubscriptionIdle {
    /// The Nautilus data client identifier.
    pub client_id: ClientId,
    /// The subscribed instrument identifier.
    pub instrument_id: InstrumentId,
    /// The subscription kind, including the complete bar type for bar subscriptions.
    pub subscription: String,
    /// The configured interval without market data.
    pub idle_timeout_secs: u64,
    /// The last local receipt time, or `None` when no market data has arrived.
    #[custom_data_field(serde)]
    pub last_data_received_ns: Option<u64>,
    /// The UNIX timestamp (nanoseconds) when inactivity is detected.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the event is created.
    pub ts_init: UnixNanos,
}

pub(crate) fn register_ib_custom_data() {
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<
        InteractiveBrokersSubscriptionIdle,
    >();
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn subscription_idle_round_trip_preserves_every_field() {
        let event = InteractiveBrokersSubscriptionIdle {
            client_id: ClientId::from("IB-DATA-17"),
            instrument_id: InstrumentId::from("AAPL=STK.SMART"),
            subscription: "trades".to_string(),
            idle_timeout_secs: 17,
            last_data_received_ns: Some(123),
            ts_event: UnixNanos::from(456_u64),
            ts_init: UnixNanos::from(789_u64),
        };
        let json = serde_json::to_string(&event).unwrap();
        let restored: InteractiveBrokersSubscriptionIdle = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);
    }
}
