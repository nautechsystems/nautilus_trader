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

//! Conversion implementations between Nautilus instruments and Cap'n Proto.

use std::error::Error;

use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::{
        BettingInstrument, BinaryOption, Cfd, Commodity, CryptoFuture, CryptoFuturesSpread,
        CryptoOption, CryptoOptionSpread, CryptoPerpetual, CurrencyPair, Equity, FuturesContract,
        FuturesSpread, IndexInstrument, InstrumentAny, OptionContract, OptionSpread,
        PerpetualContract, SyntheticInstrument, TokenizedAsset,
    },
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    FromCapnp, ToCapnp,
    conversions::{
        CapnpWriteExt, asset_class_from_capnp, asset_class_to_capnp, option_kind_from_capnp,
        option_kind_to_capnp, read_optional_from_capnp,
    },
};
use crate::{base_capnp, instruments_capnp};

fn write_unix_nanos(value: UnixNanos, mut builder: base_capnp::unix_nanos::Builder<'_>) {
    builder.set_value(*value);
}

fn read_unix_nanos<'a>(
    get: impl FnOnce() -> capnp::Result<base_capnp::unix_nanos::Reader<'a>>,
) -> Result<UnixNanos, Box<dyn Error>> {
    Ok(get()?.get_value().into())
}

fn write_optional_ustr(value: Option<Ustr>, mut set: impl FnMut(&str)) {
    if let Some(value) = value {
        set(value.as_str());
    }
}

fn read_optional_ustr<'a>(
    has: bool,
    get: impl FnOnce() -> capnp::Result<capnp::text::Reader<'a>>,
) -> Result<Option<Ustr>, Box<dyn Error>> {
    if has {
        Ok(Some(Ustr::from(get()?.to_str()?)))
    } else {
        Ok(None)
    }
}

fn read_ustr<'a>(
    get: impl FnOnce() -> capnp::Result<capnp::text::Reader<'a>>,
) -> Result<Ustr, Box<dyn Error>> {
    Ok(Ustr::from(get()?.to_str()?))
}

fn write_optional_info(info: Option<&Params>, mut set: impl FnMut(&[u8])) {
    if let Some(info) = info {
        let bytes = serde_json::to_vec(info).expect("Params serializes to JSON");
        set(&bytes);
    }
}

fn read_optional_info<'a>(
    has: bool,
    get: impl FnOnce() -> capnp::Result<&'a [u8]>,
) -> Result<Option<Params>, Box<dyn Error>> {
    if has {
        Ok(Some(serde_json::from_slice(get()?)?))
    } else {
        Ok(None)
    }
}

impl<'a> ToCapnp<'a> for BettingInstrument {
    type Builder = instruments_capnp::betting_instrument::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_event_type_id(self.event_type_id);
        builder.set_event_type_name(self.event_type_name.as_str());
        builder.set_competition_id(self.competition_id);
        builder.set_competition_name(self.competition_name.as_str());
        builder.set_event_id(self.event_id);
        builder.set_event_name(self.event_name.as_str());
        builder.set_event_country_code(self.event_country_code.as_str());
        write_unix_nanos(
            self.event_open_date,
            builder.reborrow().init_event_open_date(),
        );
        builder.set_betting_type(self.betting_type.as_str());
        builder.set_market_id(self.market_id.as_str());
        builder.set_market_name(self.market_name.as_str());
        builder.set_market_type(self.market_type.as_str());
        write_unix_nanos(
            self.market_start_time,
            builder.reborrow().init_market_start_time(),
        );
        builder.set_selection_id(self.selection_id);
        builder.set_selection_name(self.selection_name.as_str());
        builder.set_selection_handicap(self.selection_handicap);
        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for BettingInstrument {
    type Reader = instruments_capnp::betting_instrument::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            event_type_id: reader.get_event_type_id(),
            event_type_name: read_ustr(|| reader.get_event_type_name())?,
            competition_id: reader.get_competition_id(),
            competition_name: read_ustr(|| reader.get_competition_name())?,
            event_id: reader.get_event_id(),
            event_name: read_ustr(|| reader.get_event_name())?,
            event_country_code: read_ustr(|| reader.get_event_country_code())?,
            event_open_date: read_unix_nanos(|| reader.get_event_open_date())?,
            betting_type: read_ustr(|| reader.get_betting_type())?,
            market_id: read_ustr(|| reader.get_market_id())?,
            market_name: read_ustr(|| reader.get_market_name())?,
            market_type: read_ustr(|| reader.get_market_type())?,
            market_start_time: read_unix_nanos(|| reader.get_market_start_time())?,
            selection_id: reader.get_selection_id(),
            selection_name: read_ustr(|| reader.get_selection_name())?,
            selection_handicap: reader.get_selection_handicap(),
            currency: Currency::from_capnp(reader.get_currency()?)?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for BinaryOption {
    type Builder = instruments_capnp::binary_option::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        write_optional_ustr(self.event_id, |value| {
            builder.reborrow().set_event_id(value);
        });

        write_optional_ustr(self.outcome, |value| {
            builder.reborrow().set_outcome(value);
        });

        write_optional_ustr(self.description, |value| {
            builder.reborrow().set_description(value);
        });

        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for BinaryOption {
    type Reader = instruments_capnp::binary_option::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            currency: Currency::from_capnp(reader.get_currency()?)?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            event_id: read_optional_ustr(reader.has_event_id(), || reader.get_event_id())?,
            outcome: read_optional_ustr(reader.has_outcome(), || reader.get_outcome())?,
            description: read_optional_ustr(reader.has_description(), || reader.get_description())?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for Cfd {
    type Builder = instruments_capnp::cfd::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        self.base_currency
            .write_capnp(|| builder.reborrow().init_base_currency());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for Cfd {
    type Reader = instruments_capnp::cfd::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            base_currency: read_optional_from_capnp(
                || reader.has_base_currency(),
                || reader.get_base_currency(),
            )?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            lot_size: read_optional_from_capnp(|| reader.has_lot_size(), || reader.get_lot_size())?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for Commodity {
    type Builder = instruments_capnp::commodity::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for Commodity {
    type Reader = instruments_capnp::commodity::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            lot_size: read_optional_from_capnp(|| reader.has_lot_size(), || reader.get_lot_size())?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for CryptoFuture {
    type Builder = instruments_capnp::crypto_future::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        self.underlying
            .write_capnp(|| builder.reborrow().init_underlying());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        self.settlement_currency
            .write_capnp(|| builder.reborrow().init_settlement_currency());
        builder.set_is_inverse(self.is_inverse);
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for CryptoFuture {
    type Reader = instruments_capnp::crypto_future::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            underlying: Currency::from_capnp(reader.get_underlying()?)?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            settlement_currency: Currency::from_capnp(reader.get_settlement_currency()?)?,
            is_inverse: reader.get_is_inverse(),
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for CryptoFuturesSpread {
    type Builder = instruments_capnp::crypto_futures_spread::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        self.underlying
            .write_capnp(|| builder.reborrow().init_underlying());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        self.settlement_currency
            .write_capnp(|| builder.reborrow().init_settlement_currency());
        builder.set_is_inverse(self.is_inverse);
        builder.set_strategy_type(self.strategy_type.as_str());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for CryptoFuturesSpread {
    type Reader = instruments_capnp::crypto_futures_spread::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            underlying: Currency::from_capnp(reader.get_underlying()?)?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            settlement_currency: Currency::from_capnp(reader.get_settlement_currency()?)?,
            is_inverse: reader.get_is_inverse(),
            strategy_type: read_ustr(|| reader.get_strategy_type())?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for CryptoOption {
    type Builder = instruments_capnp::crypto_option::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        self.underlying
            .write_capnp(|| builder.reborrow().init_underlying());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        self.settlement_currency
            .write_capnp(|| builder.reborrow().init_settlement_currency());
        builder.set_is_inverse(self.is_inverse);
        builder.set_option_kind(option_kind_to_capnp(self.option_kind));
        self.strike_price
            .write_capnp(|| builder.reborrow().init_strike_price());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for CryptoOption {
    type Reader = instruments_capnp::crypto_option::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            underlying: Currency::from_capnp(reader.get_underlying()?)?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            settlement_currency: Currency::from_capnp(reader.get_settlement_currency()?)?,
            is_inverse: reader.get_is_inverse(),
            option_kind: option_kind_from_capnp(reader.get_option_kind()?),
            strike_price: Price::from_capnp(reader.get_strike_price()?)?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for CryptoOptionSpread {
    type Builder = instruments_capnp::crypto_option_spread::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        self.underlying
            .write_capnp(|| builder.reborrow().init_underlying());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        self.settlement_currency
            .write_capnp(|| builder.reborrow().init_settlement_currency());
        builder.set_is_inverse(self.is_inverse);
        builder.set_strategy_type(self.strategy_type.as_str());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for CryptoOptionSpread {
    type Reader = instruments_capnp::crypto_option_spread::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            underlying: Currency::from_capnp(reader.get_underlying()?)?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            settlement_currency: Currency::from_capnp(reader.get_settlement_currency()?)?,
            is_inverse: reader.get_is_inverse(),
            strategy_type: read_ustr(|| reader.get_strategy_type())?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for CryptoPerpetual {
    type Builder = instruments_capnp::crypto_perpetual::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        self.base_currency
            .write_capnp(|| builder.reborrow().init_base_currency());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        self.settlement_currency
            .write_capnp(|| builder.reborrow().init_settlement_currency());
        builder.set_is_inverse(self.is_inverse);
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for CryptoPerpetual {
    type Reader = instruments_capnp::crypto_perpetual::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            base_currency: Currency::from_capnp(reader.get_base_currency()?)?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            settlement_currency: Currency::from_capnp(reader.get_settlement_currency()?)?,
            is_inverse: reader.get_is_inverse(),
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for CurrencyPair {
    type Builder = instruments_capnp::currency_pair::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        self.base_currency
            .write_capnp(|| builder.reborrow().init_base_currency());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for CurrencyPair {
    type Reader = instruments_capnp::currency_pair::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            base_currency: Currency::from_capnp(reader.get_base_currency()?)?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: read_optional_from_capnp(|| reader.has_lot_size(), || reader.get_lot_size())?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for Equity {
    type Builder = instruments_capnp::equity::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        write_optional_ustr(self.isin, |value| {
            builder.reborrow().set_isin(value);
        });

        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        builder.set_price_precision(self.price_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for Equity {
    type Reader = instruments_capnp::equity::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            isin: read_optional_ustr(reader.has_isin(), || reader.get_isin())?,
            currency: Currency::from_capnp(reader.get_currency()?)?,
            price_precision: reader.get_price_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            lot_size: read_optional_from_capnp(|| reader.has_lot_size(), || reader.get_lot_size())?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for FuturesContract {
    type Builder = instruments_capnp::futures_contract::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        write_optional_ustr(self.exchange, |value| {
            builder.reborrow().set_exchange(value);
        });

        builder.set_underlying(self.underlying.as_str());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        builder.set_price_precision(self.price_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        builder.set_size_precision(self.size_precision);
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for FuturesContract {
    type Reader = instruments_capnp::futures_contract::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            exchange: read_optional_ustr(reader.has_exchange(), || reader.get_exchange())?,
            underlying: read_ustr(|| reader.get_underlying())?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            currency: Currency::from_capnp(reader.get_currency()?)?,
            price_precision: reader.get_price_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            size_precision: reader.get_size_precision(),
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for FuturesSpread {
    type Builder = instruments_capnp::futures_spread::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        write_optional_ustr(self.exchange, |value| {
            builder.reborrow().set_exchange(value);
        });

        builder.set_underlying(self.underlying.as_str());
        builder.set_strategy_type(self.strategy_type.as_str());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        builder.set_price_precision(self.price_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        builder.set_size_precision(self.size_precision);
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for FuturesSpread {
    type Reader = instruments_capnp::futures_spread::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            exchange: read_optional_ustr(reader.has_exchange(), || reader.get_exchange())?,
            underlying: read_ustr(|| reader.get_underlying())?,
            strategy_type: read_ustr(|| reader.get_strategy_type())?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            currency: Currency::from_capnp(reader.get_currency()?)?,
            price_precision: reader.get_price_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            size_precision: reader.get_size_precision(),
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for IndexInstrument {
    type Builder = instruments_capnp::index_instrument::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for IndexInstrument {
    type Reader = instruments_capnp::index_instrument::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            currency: Currency::from_capnp(reader.get_currency()?)?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for OptionContract {
    type Builder = instruments_capnp::option_contract::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        write_optional_ustr(self.exchange, |value| {
            builder.reborrow().set_exchange(value);
        });

        builder.set_underlying(self.underlying.as_str());
        builder.set_option_kind(option_kind_to_capnp(self.option_kind));
        self.strike_price
            .write_capnp(|| builder.reborrow().init_strike_price());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        builder.set_price_precision(self.price_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        builder.set_size_precision(self.size_precision);
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for OptionContract {
    type Reader = instruments_capnp::option_contract::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            exchange: read_optional_ustr(reader.has_exchange(), || reader.get_exchange())?,
            underlying: read_ustr(|| reader.get_underlying())?,
            option_kind: option_kind_from_capnp(reader.get_option_kind()?),
            strike_price: Price::from_capnp(reader.get_strike_price()?)?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            currency: Currency::from_capnp(reader.get_currency()?)?,
            price_precision: reader.get_price_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            size_precision: reader.get_size_precision(),
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for OptionSpread {
    type Builder = instruments_capnp::option_spread::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        write_optional_ustr(self.exchange, |value| {
            builder.reborrow().set_exchange(value);
        });

        builder.set_underlying(self.underlying.as_str());
        builder.set_strategy_type(self.strategy_type.as_str());
        write_unix_nanos(self.activation_ns, builder.reborrow().init_activation_ns());
        write_unix_nanos(self.expiration_ns, builder.reborrow().init_expiration_ns());
        self.currency
            .write_capnp(|| builder.reborrow().init_currency());
        builder.set_price_precision(self.price_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        builder.set_size_precision(self.size_precision);
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for OptionSpread {
    type Reader = instruments_capnp::option_spread::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            exchange: read_optional_ustr(reader.has_exchange(), || reader.get_exchange())?,
            underlying: read_ustr(|| reader.get_underlying())?,
            strategy_type: read_ustr(|| reader.get_strategy_type())?,
            activation_ns: read_unix_nanos(|| reader.get_activation_ns())?,
            expiration_ns: read_unix_nanos(|| reader.get_expiration_ns())?,
            currency: Currency::from_capnp(reader.get_currency()?)?,
            price_precision: reader.get_price_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            size_precision: reader.get_size_precision(),
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for PerpetualContract {
    type Builder = instruments_capnp::perpetual_contract::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_underlying(self.underlying.as_str());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        self.base_currency
            .write_capnp(|| builder.reborrow().init_base_currency());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        self.settlement_currency
            .write_capnp(|| builder.reborrow().init_settlement_currency());
        builder.set_is_inverse(self.is_inverse);
        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for PerpetualContract {
    type Reader = instruments_capnp::perpetual_contract::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            underlying: read_ustr(|| reader.get_underlying())?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            base_currency: read_optional_from_capnp(
                || reader.has_base_currency(),
                || reader.get_base_currency(),
            )?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            settlement_currency: Currency::from_capnp(reader.get_settlement_currency()?)?,
            is_inverse: reader.get_is_inverse(),
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: Quantity::from_capnp(reader.get_lot_size()?)?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for TokenizedAsset {
    type Builder = instruments_capnp::tokenized_asset::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        self.raw_symbol
            .write_capnp(|| builder.reborrow().init_raw_symbol());
        builder.set_asset_class(asset_class_to_capnp(self.asset_class));
        self.base_currency
            .write_capnp(|| builder.reborrow().init_base_currency());
        self.quote_currency
            .write_capnp(|| builder.reborrow().init_quote_currency());
        write_optional_ustr(self.isin, |value| {
            builder.reborrow().set_isin(value);
        });

        builder.set_price_precision(self.price_precision);
        builder.set_size_precision(self.size_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        self.size_increment
            .write_capnp(|| builder.reborrow().init_size_increment());
        self.multiplier
            .write_capnp(|| builder.reborrow().init_multiplier());
        self.lot_size
            .write_capnp(|| builder.reborrow().init_lot_size());
        self.margin_init
            .write_capnp(|| builder.reborrow().init_margin_init());
        self.margin_maint
            .write_capnp(|| builder.reborrow().init_margin_maint());
        self.maker_fee
            .write_capnp(|| builder.reborrow().init_maker_fee());
        self.taker_fee
            .write_capnp(|| builder.reborrow().init_taker_fee());
        self.max_quantity
            .write_capnp(|| builder.reborrow().init_max_quantity());
        self.min_quantity
            .write_capnp(|| builder.reborrow().init_min_quantity());
        self.max_notional
            .write_capnp(|| builder.reborrow().init_max_notional());
        self.min_notional
            .write_capnp(|| builder.reborrow().init_min_notional());
        self.max_price
            .write_capnp(|| builder.reborrow().init_max_price());
        self.min_price
            .write_capnp(|| builder.reborrow().init_min_price());
        write_optional_ustr(self.tick_scheme, |value| {
            builder.reborrow().set_tick_scheme(value);
        });

        write_optional_info(self.info.as_ref(), |bytes| {
            builder.reborrow().set_info(bytes);
        });

        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for TokenizedAsset {
    type Reader = instruments_capnp::tokenized_asset::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            id: InstrumentId::from_capnp(reader.get_id()?)?,
            raw_symbol: Symbol::from_capnp(reader.get_raw_symbol()?)?,
            asset_class: asset_class_from_capnp(reader.get_asset_class()?),
            base_currency: Currency::from_capnp(reader.get_base_currency()?)?,
            quote_currency: Currency::from_capnp(reader.get_quote_currency()?)?,
            isin: read_optional_ustr(reader.has_isin(), || reader.get_isin())?,
            price_precision: reader.get_price_precision(),
            size_precision: reader.get_size_precision(),
            price_increment: Price::from_capnp(reader.get_price_increment()?)?,
            size_increment: Quantity::from_capnp(reader.get_size_increment()?)?,
            multiplier: Quantity::from_capnp(reader.get_multiplier()?)?,
            lot_size: read_optional_from_capnp(|| reader.has_lot_size(), || reader.get_lot_size())?,
            margin_init: Decimal::from_capnp(reader.get_margin_init()?)?,
            margin_maint: Decimal::from_capnp(reader.get_margin_maint()?)?,
            maker_fee: Decimal::from_capnp(reader.get_maker_fee()?)?,
            taker_fee: Decimal::from_capnp(reader.get_taker_fee()?)?,
            max_quantity: read_optional_from_capnp(
                || reader.has_max_quantity(),
                || reader.get_max_quantity(),
            )?,
            min_quantity: read_optional_from_capnp(
                || reader.has_min_quantity(),
                || reader.get_min_quantity(),
            )?,
            max_notional: read_optional_from_capnp(
                || reader.has_max_notional(),
                || reader.get_max_notional(),
            )?,
            min_notional: read_optional_from_capnp(
                || reader.has_min_notional(),
                || reader.get_min_notional(),
            )?,
            max_price: read_optional_from_capnp(
                || reader.has_max_price(),
                || reader.get_max_price(),
            )?,
            min_price: read_optional_from_capnp(
                || reader.has_min_price(),
                || reader.get_min_price(),
            )?,
            tick_scheme: read_optional_ustr(reader.has_tick_scheme(), || reader.get_tick_scheme())?,
            info: read_optional_info(reader.has_info(), || reader.get_info())?,
            ts_event: read_unix_nanos(|| reader.get_ts_event())?,
            ts_init: read_unix_nanos(|| reader.get_ts_init())?,
        })
    }
}

impl<'a> ToCapnp<'a> for SyntheticInstrument {
    type Builder = instruments_capnp::synthetic_instrument::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        self.id.write_capnp(|| builder.reborrow().init_id());
        builder.set_price_precision(self.price_precision);
        self.price_increment
            .write_capnp(|| builder.reborrow().init_price_increment());
        let mut components = builder
            .reborrow()
            .init_components(self.components.len() as u32);
        for (index, component) in self.components.iter().enumerate() {
            component.to_capnp(components.reborrow().get(index as u32));
        }

        builder.set_formula(&self.formula);
        write_unix_nanos(self.ts_event, builder.reborrow().init_ts_event());
        write_unix_nanos(self.ts_init, builder.init_ts_init());
    }
}

impl<'a> FromCapnp<'a> for SyntheticInstrument {
    type Reader = instruments_capnp::synthetic_instrument::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        let id = InstrumentId::from_capnp(reader.get_id()?)?;
        let price_increment = Price::from_capnp(reader.get_price_increment()?)?;
        let components_reader = reader.get_components()?;
        let mut components = Vec::with_capacity(components_reader.len() as usize);
        for component in components_reader {
            components.push(InstrumentId::from_capnp(component)?);
        }

        let formula = reader.get_formula()?.to_str()?.to_owned();

        // The builder compiles the formula and forces Venue::synthetic() plus a derived
        // increment. Restore the stored public fields so Cap'n Proto matches serde.
        let mut instrument = Self::builder()
            .symbol(id.symbol)
            .price_precision(reader.get_price_precision())
            .components(components)
            .formula(&formula)
            .ts_event(read_unix_nanos(|| reader.get_ts_event())?)
            .ts_init(read_unix_nanos(|| reader.get_ts_init())?)
            .build()?;
        instrument.id = id;
        instrument.price_increment = price_increment;
        Ok(instrument)
    }
}

impl<'a> ToCapnp<'a> for InstrumentAny {
    type Builder = instruments_capnp::instrument_any::Builder<'a>;

    fn to_capnp(&self, builder: Self::Builder) {
        match self {
            Self::Betting(value) => value.to_capnp(builder.init_betting()),
            Self::BinaryOption(value) => value.to_capnp(builder.init_binary_option()),
            Self::Cfd(value) => value.to_capnp(builder.init_cfd()),
            Self::Commodity(value) => value.to_capnp(builder.init_commodity()),
            Self::CryptoFuture(value) => value.to_capnp(builder.init_crypto_future()),
            Self::CryptoFuturesSpread(value) => {
                value.to_capnp(builder.init_crypto_futures_spread());
            }
            Self::CryptoOption(value) => value.to_capnp(builder.init_crypto_option()),
            Self::CryptoOptionSpread(value) => value.to_capnp(builder.init_crypto_option_spread()),
            Self::CryptoPerpetual(value) => value.to_capnp(builder.init_crypto_perpetual()),
            Self::CurrencyPair(value) => value.to_capnp(builder.init_currency_pair()),
            Self::Equity(value) => value.to_capnp(builder.init_equity()),
            Self::FuturesContract(value) => value.to_capnp(builder.init_futures_contract()),
            Self::FuturesSpread(value) => value.to_capnp(builder.init_futures_spread()),
            Self::IndexInstrument(value) => value.to_capnp(builder.init_index_instrument()),
            Self::OptionContract(value) => value.to_capnp(builder.init_option_contract()),
            Self::OptionSpread(value) => value.to_capnp(builder.init_option_spread()),
            Self::PerpetualContract(value) => value.to_capnp(builder.init_perpetual_contract()),
            Self::TokenizedAsset(value) => value.to_capnp(builder.init_tokenized_asset()),
        }
    }
}

impl<'a> FromCapnp<'a> for InstrumentAny {
    type Reader = instruments_capnp::instrument_any::Reader<'a>;

    fn from_capnp(reader: Self::Reader) -> Result<Self, Box<dyn Error>> {
        match reader.which()? {
            instruments_capnp::instrument_any::Betting(reader) => {
                Ok(Self::Betting(BettingInstrument::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::BinaryOption(reader) => {
                Ok(Self::BinaryOption(BinaryOption::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::Cfd(reader) => {
                Ok(Self::Cfd(Cfd::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::Commodity(reader) => {
                Ok(Self::Commodity(Commodity::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::CryptoFuture(reader) => {
                Ok(Self::CryptoFuture(CryptoFuture::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::CryptoFuturesSpread(reader) => Ok(
                Self::CryptoFuturesSpread(CryptoFuturesSpread::from_capnp(reader?)?),
            ),
            instruments_capnp::instrument_any::CryptoOption(reader) => {
                Ok(Self::CryptoOption(CryptoOption::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::CryptoOptionSpread(reader) => Ok(
                Self::CryptoOptionSpread(CryptoOptionSpread::from_capnp(reader?)?),
            ),
            instruments_capnp::instrument_any::CryptoPerpetual(reader) => {
                Ok(Self::CryptoPerpetual(CryptoPerpetual::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::CurrencyPair(reader) => {
                Ok(Self::CurrencyPair(CurrencyPair::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::Equity(reader) => {
                Ok(Self::Equity(Equity::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::FuturesContract(reader) => {
                Ok(Self::FuturesContract(FuturesContract::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::FuturesSpread(reader) => {
                Ok(Self::FuturesSpread(FuturesSpread::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::IndexInstrument(reader) => {
                Ok(Self::IndexInstrument(IndexInstrument::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::OptionContract(reader) => {
                Ok(Self::OptionContract(OptionContract::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::OptionSpread(reader) => {
                Ok(Self::OptionSpread(OptionSpread::from_capnp(reader?)?))
            }
            instruments_capnp::instrument_any::PerpetualContract(reader) => Ok(
                Self::PerpetualContract(PerpetualContract::from_capnp(reader?)?),
            ),
            instruments_capnp::instrument_any::TokenizedAsset(reader) => {
                Ok(Self::TokenizedAsset(TokenizedAsset::from_capnp(reader?)?))
            }
        }
    }
}

/// Serializes an [`InstrumentAny`] to Cap'n Proto bytes.
///
/// # Errors
///
/// Returns an error if Cap'n Proto serialization fails.
pub fn serialize_instrument_any(instrument: &InstrumentAny) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<instruments_capnp::instrument_any::Builder>();
    instrument.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message)?;
    Ok(bytes)
}

/// Deserializes an [`InstrumentAny`] from Cap'n Proto bytes.
///
/// # Errors
///
/// Returns an error if Cap'n Proto deserialization fails.
pub fn deserialize_instrument_any(bytes: &[u8]) -> Result<InstrumentAny, Box<dyn Error>> {
    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())?;
    let root = reader.get_root::<instruments_capnp::instrument_any::Reader>()?;
    InstrumentAny::from_capnp(root)
}
