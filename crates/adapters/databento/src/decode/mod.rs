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

//! Databento message decoding functions.
//!
//! # Sentinel Values
//!
//! Databento uses sentinel values to represent undefined/null fields:
//!
//! | Sentinel          | Value      | Usage                       |
//! |-------------------|------------|-----------------------------|
//! | `UNDEF_PRICE`     | `i64::MAX` | Undefined price fields.     |
//! | `UNDEF_TIMESTAMP` | `u64::MAX` | Undefined timestamp fields. |
//!
//! # Fields Potentially Undefined
//!
//! According to Databento documentation, the following fields can contain sentinel values:
//!
//! | Message Type       | Field                          | Handling                           |
//! |--------------------|--------------------------------|------------------------------------|
//! | `MboMsg`           | `price`                        | Passed through as `PRICE_UNDEF`.   |
//! | `TradeMsg`         | `price`                        | Passed through as `PRICE_UNDEF`.   |
//! | `OhlcvMsg`         | `open`, `high`, `low`, `close` | Passed through as `PRICE_UNDEF`.   |
//! | `Mbp1Msg`          | `bid_px`, `ask_px`             | Quote skipped if either undefined. |
//! | `InstrumentDefMsg` | `activation`                   | Defaults to 0 (epoch).             |
//! | `InstrumentDefMsg` | `expiration`                   | Returns error if undefined.        |
//! | `InstrumentDefMsg` | `strike_price`                 | Returns error if undefined.        |
//!
//! # References
//!
//! - [`UNDEF_PRICE`](https://docs.rs/dbn/latest/dbn/constant.UNDEF_PRICE.html)
//! - [`UNDEF_TIMESTAMP`](https://docs.rs/dbn/latest/dbn/constant.UNDEF_TIMESTAMP.html)
//! - [Databento DBN Schema](https://databento.com/docs/schemas)

mod custom;
mod instruments;
mod market_data;
mod primitives;

pub use custom::{decode_imbalance_msg, decode_statistics_msg, is_supported_stat_type};
pub use instruments::{
    decode_equity, decode_futures_contract, decode_futures_spread, decode_instrument_def_msg,
    decode_option_contract, decode_option_spread,
};
pub use market_data::{
    decode_bar_type, decode_bbo_msg, decode_cbbo_msg, decode_cmbp1_msg, decode_mbo_msg,
    decode_mbp1_msg, decode_mbp10_msg, decode_ohlcv_msg, decode_record, decode_status_msg,
    decode_tbbo_msg, decode_tcbbo_msg, decode_trade_msg, decode_ts_event_adjustment,
};
pub use primitives::{
    decode_lot_size, decode_multiplier, decode_optional_price, decode_optional_quantity,
    decode_optional_timestamp, decode_price, decode_price_increment, decode_price_or_undef,
    decode_quantity, decode_timestamp, parse_aggressor_side, parse_book_action, parse_cfi_iso10926,
    parse_option_kind, parse_optional_bool, parse_order_side, parse_status_reason,
    parse_status_trading_event, precision_from_raw,
};

#[cfg(test)]
mod tests;
