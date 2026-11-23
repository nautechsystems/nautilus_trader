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

use nautilus_common::messages::{DataEvent, ExecutionEvent, ExecutionReport, data::DataCommand};
use nautilus_core::UnixNanos;

pub(super) fn stub_data_event() -> DataEvent {
    use nautilus_model::instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt};

    DataEvent::Data(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()).into())
}

pub(super) fn stub_data_command() -> DataCommand {
    use nautilus_common::messages::data::{SubscribeCommand, subscribe::SubscribeInstruments};
    use nautilus_core::UUID4;
    use nautilus_model::identifiers::Venue;

    DataCommand::Subscribe(SubscribeCommand::Instruments(SubscribeInstruments::new(
        None,
        Venue::from("TEST"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

pub(super) fn stub_exec_event() -> ExecutionEvent {
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide},
        identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
        reports::FillReport,
        types::{Money, Price, Quantity},
    };

    ExecutionEvent::Report(ExecutionReport::Fill(Box::new(FillReport::new(
        AccountId::from("TEST-001"),
        InstrumentId::from("TEST.VENUE"),
        VenueOrderId::from("V-001"),
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("100.0"),
        Money::from("0.01 USD"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    ))))
}
