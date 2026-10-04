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

use derive_builder::Builder;
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::identifiers::{
    ClientId, ClientOrderId, InstrumentId, TraderId, Venue, VenueOrderId,
};
use serde::{Deserialize, Serialize};

use crate::enums::LogLevel;

const fn default_report_log_level() -> LogLevel {
    LogLevel::Info
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct GenerateOrderStatusReport {
    #[builder(default = "UUID4::new()")]
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    #[builder(default)]
    pub instrument_id: Option<InstrumentId>,
    #[builder(default)]
    pub client_order_id: Option<ClientOrderId>,
    #[builder(default)]
    pub venue_order_id: Option<VenueOrderId>,
    #[builder(default)]
    pub params: Option<Params>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<UUID4>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl GenerateOrderStatusReport {
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            client_order_id,
            venue_order_id,
            params,
            correlation_id,
            causation_id: None,
        }
    }
}

impl Display for GenerateOrderStatusReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={:?}, client_order_id={:?}, venue_order_id={:?}, command_id={})",
            stringify!(GenerateOrderStatusReport),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct GenerateOrderStatusReports {
    #[builder(default = "UUID4::new()")]
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub open_only: bool,
    #[builder(default)]
    pub instrument_id: Option<InstrumentId>,
    #[builder(default)]
    pub start: Option<UnixNanos>,
    #[builder(default)]
    pub end: Option<UnixNanos>,
    #[builder(default)]
    pub params: Option<Params>,
    /// The log level for receipt logging.
    #[builder(default = "default_report_log_level()")]
    #[serde(default = "default_report_log_level")]
    pub log_receipt_level: LogLevel,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<UUID4>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl GenerateOrderStatusReports {
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        open_only: bool,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            open_only,
            instrument_id,
            start,
            end,
            params,
            log_receipt_level: LogLevel::Info,
            correlation_id,
            causation_id: None,
        }
    }
}

impl Display for GenerateOrderStatusReports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(open_only={}, instrument_id={:?}, command_id={})",
            stringify!(GenerateOrderStatusReports),
            self.open_only,
            self.instrument_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct GenerateFillReports {
    #[builder(default = "UUID4::new()")]
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    #[builder(default)]
    pub instrument_id: Option<InstrumentId>,
    #[builder(default)]
    pub venue_order_id: Option<VenueOrderId>,
    #[builder(default)]
    pub start: Option<UnixNanos>,
    #[builder(default)]
    pub end: Option<UnixNanos>,
    #[builder(default)]
    pub params: Option<Params>,
    /// The log level for receipt logging.
    #[builder(default = "default_report_log_level()")]
    #[serde(default = "default_report_log_level")]
    pub log_receipt_level: LogLevel,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<UUID4>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl GenerateFillReports {
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<VenueOrderId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            venue_order_id,
            start,
            end,
            params,
            log_receipt_level: LogLevel::Info,
            correlation_id,
            causation_id: None,
        }
    }
}

impl Display for GenerateFillReports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={:?}, venue_order_id={:?}, command_id={})",
            stringify!(GenerateFillReports),
            self.instrument_id,
            self.venue_order_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct GeneratePositionStatusReports {
    #[builder(default = "UUID4::new()")]
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    #[builder(default)]
    pub instrument_id: Option<InstrumentId>,
    #[builder(default)]
    pub start: Option<UnixNanos>,
    #[builder(default)]
    pub end: Option<UnixNanos>,
    #[builder(default)]
    pub params: Option<Params>,
    /// The log level for receipt logging.
    #[builder(default = "default_report_log_level()")]
    #[serde(default = "default_report_log_level")]
    pub log_receipt_level: LogLevel,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<UUID4>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl GeneratePositionStatusReports {
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            start,
            end,
            params,
            log_receipt_level: LogLevel::Info,
            correlation_id,
            causation_id: None,
        }
    }
}

impl Display for GeneratePositionStatusReports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={:?}, command_id={})",
            stringify!(GeneratePositionStatusReports),
            self.instrument_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Builder)]
pub struct GenerateExecutionMassStatus {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    #[builder(default)]
    pub venue: Option<Venue>,
    #[builder(default = "UUID4::new()")]
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    #[builder(default)]
    pub params: Option<Params>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<UUID4>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl GenerateExecutionMassStatus {
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: ClientId,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
            correlation_id,
            causation_id: None,
        }
    }
}

impl Display for GenerateExecutionMassStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, client_id={}, venue={:?}, command_id={})",
            stringify!(GenerateExecutionMassStatus),
            self.trader_id,
            self.client_id,
            self.venue,
            self.command_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const COMMAND_ID: &str = "00000000-0000-4000-8000-000000000001";

    fn command_id() -> UUID4 {
        UUID4::from(COMMAND_ID)
    }

    fn correlation_id() -> UUID4 {
        UUID4::from("00000000-0000-4000-8000-000000000002")
    }

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("ETHUSDT.BINANCE")
    }

    fn params() -> Params {
        let mut params = Params::new();
        params.insert("depth".into(), "10".into());
        params
    }

    #[rstest]
    fn test_generate_order_status_report_assigns_every_field() {
        let command = GenerateOrderStatusReport::new(
            command_id(),
            UnixNanos::from(3),
            Some(instrument_id()),
            Some(ClientOrderId::from("O-1")),
            Some(VenueOrderId::from("V-1")),
            Some(params()),
            Some(correlation_id()),
        );

        assert_eq!(command.command_id, command_id());
        assert_eq!(command.ts_init, UnixNanos::from(3));
        assert_eq!(command.instrument_id, Some(instrument_id()));
        assert_eq!(command.client_order_id, Some(ClientOrderId::from("O-1")));
        assert_eq!(command.venue_order_id, Some(VenueOrderId::from("V-1")));
        assert_eq!(command.params, Some(params()));
        assert_eq!(command.correlation_id, Some(correlation_id()));
        assert_eq!(command.causation_id, None);
        assert_eq!(
            command.to_string(),
            format!(
                "GenerateOrderStatusReport(instrument_id=Some(\"ETHUSDT.BINANCE\"), \
                 client_order_id=Some(\"O-1\"), venue_order_id=Some(\"V-1\"), command_id={COMMAND_ID})"
            )
        );
    }

    #[rstest]
    fn test_generate_order_status_reports_assigns_every_field() {
        let command = GenerateOrderStatusReports::new(
            command_id(),
            UnixNanos::from(5),
            true,
            Some(instrument_id()),
            Some(UnixNanos::from(7)),
            Some(UnixNanos::from(11)),
            Some(params()),
            Some(correlation_id()),
        );

        assert_eq!(command.command_id, command_id());
        assert_eq!(command.ts_init, UnixNanos::from(5));
        assert!(command.open_only);
        assert_eq!(command.instrument_id, Some(instrument_id()));
        assert_eq!(command.start, Some(UnixNanos::from(7)));
        assert_eq!(command.end, Some(UnixNanos::from(11)));
        assert_eq!(command.params, Some(params()));
        assert_eq!(command.log_receipt_level, LogLevel::Info);
        assert_eq!(command.correlation_id, Some(correlation_id()));
        assert_eq!(command.causation_id, None);
        assert_eq!(
            command.to_string(),
            format!(
                "GenerateOrderStatusReports(open_only=true, \
                 instrument_id=Some(\"ETHUSDT.BINANCE\"), command_id={COMMAND_ID})"
            )
        );
    }

    #[rstest]
    fn test_generate_fill_reports_assigns_every_field() {
        let command = GenerateFillReports::new(
            command_id(),
            UnixNanos::from(13),
            Some(instrument_id()),
            Some(VenueOrderId::from("V-2")),
            Some(UnixNanos::from(17)),
            Some(UnixNanos::from(19)),
            Some(params()),
            Some(correlation_id()),
        );

        assert_eq!(command.command_id, command_id());
        assert_eq!(command.ts_init, UnixNanos::from(13));
        assert_eq!(command.instrument_id, Some(instrument_id()));
        assert_eq!(command.venue_order_id, Some(VenueOrderId::from("V-2")));
        assert_eq!(command.start, Some(UnixNanos::from(17)));
        assert_eq!(command.end, Some(UnixNanos::from(19)));
        assert_eq!(command.params, Some(params()));
        assert_eq!(command.log_receipt_level, LogLevel::Info);
        assert_eq!(command.correlation_id, Some(correlation_id()));
        assert_eq!(command.causation_id, None);
        assert_eq!(
            command.to_string(),
            format!(
                "GenerateFillReports(instrument_id=Some(\"ETHUSDT.BINANCE\"), \
                 venue_order_id=Some(\"V-2\"), command_id={COMMAND_ID})"
            )
        );
    }

    #[rstest]
    fn test_generate_position_status_reports_assigns_every_field() {
        let command = GeneratePositionStatusReports::new(
            command_id(),
            UnixNanos::from(23),
            Some(instrument_id()),
            Some(UnixNanos::from(29)),
            Some(UnixNanos::from(31)),
            Some(params()),
            Some(correlation_id()),
        );

        assert_eq!(command.command_id, command_id());
        assert_eq!(command.ts_init, UnixNanos::from(23));
        assert_eq!(command.instrument_id, Some(instrument_id()));
        assert_eq!(command.start, Some(UnixNanos::from(29)));
        assert_eq!(command.end, Some(UnixNanos::from(31)));
        assert_eq!(command.params, Some(params()));
        assert_eq!(command.log_receipt_level, LogLevel::Info);
        assert_eq!(command.correlation_id, Some(correlation_id()));
        assert_eq!(command.causation_id, None);
        assert_eq!(
            command.to_string(),
            format!(
                "GeneratePositionStatusReports(instrument_id=Some(\"ETHUSDT.BINANCE\"), \
                 command_id={COMMAND_ID})"
            )
        );
    }

    #[rstest]
    fn test_generate_execution_mass_status_assigns_every_field() {
        let command = GenerateExecutionMassStatus::new(
            TraderId::from("TESTER-001"),
            ClientId::from("BINANCE"),
            Some(Venue::from("BINANCE")),
            command_id(),
            UnixNanos::from(37),
            Some(params()),
            Some(correlation_id()),
        );

        assert_eq!(command.trader_id, TraderId::from("TESTER-001"));
        assert_eq!(command.client_id, ClientId::from("BINANCE"));
        assert_eq!(command.venue, Some(Venue::from("BINANCE")));
        assert_eq!(command.command_id, command_id());
        assert_eq!(command.ts_init, UnixNanos::from(37));
        assert_eq!(command.params, Some(params()));
        assert_eq!(command.correlation_id, Some(correlation_id()));
        assert_eq!(command.causation_id, None);
        assert_eq!(
            command.to_string(),
            format!(
                "GenerateExecutionMassStatus(trader_id=TESTER-001, client_id=BINANCE, \
                 venue=Some(\"BINANCE\"), command_id={COMMAND_ID})"
            )
        );
    }

    #[rstest]
    fn test_builders_default_the_optional_fields() {
        let command = GenerateFillReportsBuilder::default()
            .ts_init(UnixNanos::from(41))
            .build()
            .unwrap();

        assert_eq!(command.ts_init, UnixNanos::from(41));
        assert_eq!(command.instrument_id, None);
        assert_eq!(command.venue_order_id, None);
        assert_eq!(command.start, None);
        assert_eq!(command.end, None);
        assert_eq!(command.params, None);
        assert_eq!(command.log_receipt_level, LogLevel::Info);
        assert_eq!(command.correlation_id, None);
        assert_eq!(command.causation_id, None);
    }

    #[rstest]
    fn test_report_commands_round_trip_through_json() {
        let command = GenerateOrderStatusReports::new(
            command_id(),
            UnixNanos::from(43),
            false,
            None,
            None,
            None,
            None,
            None,
        );

        let json = serde_json::to_string(&command).unwrap();

        assert_eq!(
            serde_json::from_str::<GenerateOrderStatusReports>(&json).unwrap(),
            command
        );
    }
}
