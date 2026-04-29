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

pub mod mock_server;

use std::{ffi::c_char, sync::Arc};

use databento::dbn::{
    self, ASSET_CSTR_LEN, ErrorMsg, FlagSet, ImbalanceMsg, InstrumentDefMsg, MboMsg, Mbp1Msg,
    Mbp10Msg, OhlcvMsg, SYMBOL_CSTR_LEN, StatMsg, StatusMsg, SymbolMappingMsg, SystemMsg, TradeMsg,
    enums::rtype,
    record::{BidAskPair, RecordHeader},
};
use indexmap::IndexMap;
use nautilus_core::AtomicMap;
use nautilus_databento::{
    common::Credential,
    live::{DatabentoFeedHandler, DatabentoMessage, HandlerCommand},
};
use nautilus_model::identifiers::Venue;

pub const TEST_KEY: &str = "32-character-with-lots-of-filler";
pub const TEST_DATASET: &str = "GLBX.MDP3";
pub const PUBLISHER_ID: u16 = 1;

pub fn publisher_venue_map() -> IndexMap<u16, Venue> {
    let mut map = IndexMap::new();
    map.insert(PUBLISHER_ID, Venue::from("GLBX"));
    map
}

#[derive(Default)]
pub struct TestHandlerConfig {
    pub use_exchange_as_venue: bool,
    pub bars_timestamp_on_close: bool,
    pub reconnect_timeout_mins: Option<u64>,
}

pub fn create_test_handler(
    addr: &str,
    dataset: &str,
) -> (
    tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    tokio::sync::mpsc::Receiver<DatabentoMessage>,
    DatabentoFeedHandler,
) {
    create_test_handler_with_config(addr, dataset, &TestHandlerConfig::default())
}

pub fn create_test_handler_with_config(
    addr: &str,
    dataset: &str,
    config: &TestHandlerConfig,
) -> (
    tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    tokio::sync::mpsc::Receiver<DatabentoMessage>,
    DatabentoFeedHandler,
) {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (msg_tx, msg_rx) = tokio::sync::mpsc::channel(100);

    let handler = DatabentoFeedHandler::new(
        Credential::new(TEST_KEY),
        dataset.to_string(),
        cmd_rx,
        msg_tx,
        publisher_venue_map(),
        Arc::new(AtomicMap::new()),
        config.use_exchange_as_venue,
        config.bars_timestamp_on_close,
        config.reconnect_timeout_mins,
    )
    .with_gateway_addr(addr.to_string());

    (cmd_tx, msg_rx, handler)
}

fn str_to_cchar_array<const N: usize>(s: &str) -> [c_char; N] {
    let mut arr = [0 as c_char; N];
    for (i, byte) in s.bytes().enumerate() {
        if i >= N - 1 {
            break;
        }
        arr[i] = byte as c_char;
    }
    arr
}

pub fn symbol_mapping_msg(instrument_id: u32, raw_symbol: &str) -> SymbolMappingMsg {
    SymbolMappingMsg {
        hd: RecordHeader::new::<SymbolMappingMsg>(
            rtype::SYMBOL_MAPPING,
            PUBLISHER_ID,
            instrument_id,
            1_000_000_000,
        ),
        stype_in: dbn::SType::InstrumentId as u8,
        stype_in_symbol: str_to_cchar_array::<SYMBOL_CSTR_LEN>(&instrument_id.to_string()),
        stype_out: dbn::SType::RawSymbol as u8,
        stype_out_symbol: str_to_cchar_array::<SYMBOL_CSTR_LEN>(raw_symbol),
        start_ts: 1_000_000_000,
        end_ts: u64::MAX,
    }
}

pub fn trade_msg(instrument_id: u32, price: i64, size: u32) -> TradeMsg {
    TradeMsg {
        hd: RecordHeader::new::<TradeMsg>(rtype::MBP_0, PUBLISHER_ID, instrument_id, 1_000_000_000),
        price,
        size,
        action: b'T' as c_char,
        side: b'A' as c_char,
        flags: FlagSet::new(128), // F_LAST
        depth: 0,
        ts_recv: 1_000_000_000,
        ts_in_delta: 0,
        sequence: 1,
    }
}

pub fn mbp1_msg(instrument_id: u32, bid_px: i64, ask_px: i64, action: u8) -> Mbp1Msg {
    Mbp1Msg {
        hd: RecordHeader::new::<Mbp1Msg>(rtype::MBP_1, PUBLISHER_ID, instrument_id, 1_000_000_000),
        price: bid_px,
        size: 10,
        action: action as c_char,
        side: b'N' as c_char,
        flags: FlagSet::new(128), // F_LAST
        depth: 0,
        ts_recv: 1_000_000_000,
        ts_in_delta: 0,
        sequence: 1,
        levels: [BidAskPair {
            bid_px,
            ask_px,
            bid_sz: 100,
            ask_sz: 50,
            bid_ct: 5,
            ask_ct: 3,
        }],
    }
}

pub fn mbp10_msg(instrument_id: u32) -> Mbp10Msg {
    let levels = std::array::from_fn::<BidAskPair, 10, _>(|i| {
        let offset = (i as i64) * 1_000_000_000;
        BidAskPair {
            bid_px: 100_000_000_000 - offset, // 100.00 descending
            ask_px: 101_000_000_000 + offset, // 101.00 ascending
            bid_sz: 100 - i as u32,
            ask_sz: 50 + i as u32,
            bid_ct: 5,
            ask_ct: 3,
        }
    });

    Mbp10Msg {
        hd: RecordHeader::new::<Mbp10Msg>(
            rtype::MBP_10,
            PUBLISHER_ID,
            instrument_id,
            1_000_000_000,
        ),
        price: 100_500_000_000,
        size: 10,
        action: b'A' as c_char,
        side: b'N' as c_char,
        flags: FlagSet::new(128), // F_LAST
        depth: 0,
        ts_recv: 1_000_000_000,
        ts_in_delta: 0,
        sequence: 1,
        levels,
    }
}

pub fn mbo_msg(instrument_id: u32, action: u8, side: u8, flags: u8, price: i64) -> MboMsg {
    mbo_msg_with_ts(instrument_id, action, side, flags, price, 1_000_000_000)
}

pub fn mbo_msg_with_ts(
    instrument_id: u32,
    action: u8,
    side: u8,
    flags: u8,
    price: i64,
    ts_event: u64,
) -> MboMsg {
    MboMsg {
        hd: RecordHeader::new::<MboMsg>(rtype::MBO, PUBLISHER_ID, instrument_id, ts_event),
        order_id: 1,
        price,
        size: 10,
        flags: FlagSet::new(flags),
        channel_id: 0,
        action: action as c_char,
        side: side as c_char,
        ts_recv: ts_event,
        ts_in_delta: 0,
        sequence: 1,
    }
}

pub fn ohlcv_msg(instrument_id: u32) -> OhlcvMsg {
    OhlcvMsg {
        hd: RecordHeader::new::<OhlcvMsg>(
            rtype::OHLCV_1S,
            PUBLISHER_ID,
            instrument_id,
            1_000_000_000,
        ),
        open: 100_000_000_000,
        high: 102_000_000_000,
        low: 99_000_000_000,
        close: 101_000_000_000,
        volume: 1000,
    }
}

pub fn status_msg(instrument_id: u32) -> StatusMsg {
    StatusMsg {
        hd: RecordHeader::new::<StatusMsg>(
            rtype::STATUS,
            PUBLISHER_ID,
            instrument_id,
            1_000_000_000,
        ),
        ts_recv: 1_000_000_000,
        action: 1, // Trading
        reason: 0,
        trading_event: 0,
        is_trading: b'Y' as c_char,
        is_quoting: b'Y' as c_char,
        is_short_sell_restricted: b'~' as c_char,
        _reserved: [0u8; 7],
    }
}

#[expect(
    clippy::field_reassign_with_default,
    reason = "conditional fields (options) prevent struct init syntax"
)]
pub fn instrument_def_msg(instrument_id: u32, instrument_class: u8) -> InstrumentDefMsg {
    let mut msg = InstrumentDefMsg::default();
    msg.hd = RecordHeader::new::<InstrumentDefMsg>(
        rtype::INSTRUMENT_DEF,
        PUBLISHER_ID,
        instrument_id,
        1_000_000_000,
    );
    msg.ts_recv = 1_000_000_000;
    msg.min_price_increment = 10_000_000; // 0.01
    msg.unit_of_measure_qty = 1_000_000_000; // 1.0
    msg.min_lot_size_round_lot = 1;
    msg.currency = str_to_cchar_array::<4>("USD");
    msg.exchange = str_to_cchar_array::<5>("XCME");
    msg.asset = str_to_cchar_array::<ASSET_CSTR_LEN>("ES");
    msg.instrument_class = instrument_class as c_char;

    // Expiration is required by the decoder (errors on UNDEF_TIMESTAMP)
    msg.expiration = 2_000_000_000_000_000_000;

    // CFI code determines instrument type mapping
    msg.cfi = match instrument_class {
        b'K' => str_to_cchar_array::<7>("EXXXXX"),
        b'F' => str_to_cchar_array::<7>("FXXXXX"),
        b'C' => str_to_cchar_array::<7>("OCXXXX"),
        b'P' => str_to_cchar_array::<7>("OPXXXX"),
        _ => str_to_cchar_array::<7>("XXXXXX"),
    };

    // Options require valid strike price
    if instrument_class == b'C' || instrument_class == b'P' {
        msg.strike_price = 100_000_000_000;
        msg.strike_price_currency = str_to_cchar_array::<4>("USD");
        msg.underlying = str_to_cchar_array::<21>("ES");
    }

    msg
}

pub fn imbalance_msg(instrument_id: u32) -> ImbalanceMsg {
    ImbalanceMsg {
        hd: RecordHeader::new::<ImbalanceMsg>(
            rtype::IMBALANCE,
            PUBLISHER_ID,
            instrument_id,
            1_000_000_000,
        ),
        ts_recv: 1_000_000_000,
        ref_price: 100_000_000_000,
        cont_book_clr_price: 100_500_000_000,
        auct_interest_clr_price: 100_250_000_000,
        paired_qty: 1000,
        total_imbalance_qty: 500,
        side: b'B' as c_char,
        ..Default::default()
    }
}

pub fn statistics_msg(instrument_id: u32) -> StatMsg {
    StatMsg {
        hd: RecordHeader::new::<StatMsg>(
            rtype::STATISTICS,
            PUBLISHER_ID,
            instrument_id,
            1_000_000_000,
        ),
        ts_recv: 1_000_000_000,
        ts_ref: 1_000_000_000,
        stat_type: 1,     // OpeningPrice
        update_action: 1, // Added
        price: 100_000_000_000,
        ..Default::default()
    }
}

pub fn error_msg(message: &str) -> ErrorMsg {
    let mut err = [0 as c_char; 302];

    for (i, byte) in message.bytes().enumerate() {
        if i >= 301 {
            break;
        }
        err[i] = byte as c_char;
    }
    ErrorMsg {
        hd: RecordHeader::new::<ErrorMsg>(rtype::ERROR, 0, 0, 1_000_000_000),
        err,
        code: 0,
        is_last: 1,
    }
}

pub fn system_msg(message: &str, code: u8) -> SystemMsg {
    let mut msg_bytes = [0 as c_char; 303];

    for (i, byte) in message.bytes().enumerate() {
        if i >= 302 {
            break;
        }
        msg_bytes[i] = byte as c_char;
    }
    SystemMsg {
        hd: RecordHeader::new::<SystemMsg>(rtype::SYSTEM, 0, 0, 1_000_000_000),
        msg: msg_bytes,
        code,
    }
}
