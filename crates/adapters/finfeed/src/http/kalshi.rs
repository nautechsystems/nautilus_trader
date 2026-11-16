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

//! Public Kalshi metadata used by FinFeed instrument definitions.

use jiff::Timestamp;
use nautilus_core::{Params, UnixNanos, serialization::decimal};
use nautilus_model::{
    enums::AssetClass,
    identifiers::InstrumentId,
    instruments::{
        BinaryOption,
        tick_scheme::{TickScheme, TieredTickScheme, get_tick_scheme, register_tick_scheme},
    },
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use crate::parse::timestamp;

const PRICE_PRECISION: u8 = 6;
const SIZE_PRECISION: u8 = 2;

#[derive(Debug, Deserialize)]
pub(crate) struct MarketResponse {
    pub(super) market: Market,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EventResponse {
    pub(super) event: Event,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Market {
    pub(super) ticker: String,
    pub(super) event_ticker: String,
    market_type: String,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    notional_value_dollars: Decimal,
    settlement_bounds_type: String,
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    settlement_floor_dollars: Option<Decimal>,
    mve_collection_ticker: Option<String>,
    mve_selected_legs: Option<Vec<Value>>,
    open_time: Timestamp,
    pub(super) close_time: Timestamp,
    latest_expiration_time: Timestamp,
    updated_time: Option<Timestamp>,
    pub(super) status: String,
    price_ranges: Vec<PriceRange>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Event {
    pub(super) event_ticker: String,
    collateral_return_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PriceRange {
    #[serde(deserialize_with = "decimal::deserialize_json")]
    start: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    end: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    step: Decimal,
}

#[derive(Debug)]
struct PriceGrid {
    name: String,
    scheme: TieredTickScheme,
    increment: Price,
}

pub(super) fn parse_definition(
    market: &Market,
    event: &Event,
    instrument_id: InstrumentId,
    is_no: bool,
    ts_init: UnixNanos,
) -> anyhow::Result<BinaryOption> {
    anyhow::ensure!(
        market.market_type == "binary"
            && market.notional_value_dollars == Decimal::ONE
            && market.settlement_bounds_type == "default"
            && event.collateral_return_type.is_empty()
            && market
                .settlement_floor_dollars
                .is_none_or(|floor| floor.is_zero())
            && market
                .mve_collection_ticker
                .as_ref()
                .is_none_or(String::is_empty)
            && market.mve_selected_legs.as_ref().is_none_or(Vec::is_empty),
        "unsupported market economics or family"
    );
    let grid = PriceGrid::new(&market.price_ranges)?;
    let grid = if is_no { grid.complement()? } else { grid };
    let tick_scheme = grid.register()?;
    let activation_ns = timestamp(market.open_time)?;
    let expiration_ns = timestamp(market.latest_expiration_time)?;
    anyhow::ensure!(
        activation_ns < expiration_ns,
        "invalid instrument lifecycle interval"
    );
    timestamp(market.close_time)?;
    let ts_event = market
        .updated_time
        .map(timestamp)
        .transpose()?
        .unwrap_or(ts_init);

    let info = Params::new(
        [
            ("market".into(), serde_json::to_value(market)?),
            ("event".into(), serde_json::to_value(event)?),
        ]
        .into(),
    );

    let size_increment =
        Quantity::from_decimal_dp(Decimal::new(1, u32::from(SIZE_PRECISION)), SIZE_PRECISION)?;
    Ok(BinaryOption::builder()
        .instrument_id(instrument_id)
        .raw_symbol(instrument_id.symbol)
        .asset_class(AssetClass::Alternative)
        .currency(Currency::USD())
        .activation_ns(activation_ns)
        .expiration_ns(expiration_ns)
        .price_precision(PRICE_PRECISION)
        .size_precision(SIZE_PRECISION)
        .price_increment(grid.increment())
        .size_increment(size_increment)
        .min_quantity(size_increment)
        .event_id(Ustr::from(&market.event_ticker))
        .tick_scheme(tick_scheme)
        .info(info)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()?)
}

impl PriceGrid {
    fn new(ranges: &[PriceRange]) -> anyhow::Result<Self> {
        anyhow::ensure!(!ranges.is_empty(), "missing price ranges");
        let mut name = String::from("FINFEED_KALSHI");
        let mut ticks = Vec::new();
        let mut quantum = Decimal::ZERO;
        let mut previous_end = Decimal::ZERO;

        for range in ranges {
            let start = range.start;
            let end = range.end;
            let step = range.step;
            anyhow::ensure!(
                start >= previous_end
                    && start >= Decimal::ZERO
                    && start < end
                    && end <= Decimal::ONE,
                "invalid or overlapping price range"
            );
            anyhow::ensure!(step > Decimal::ZERO, "invalid price range step");

            for value in [start, end, step] {
                anyhow::ensure!(
                    value.normalize().scale() <= u32::from(PRICE_PRECISION),
                    "price range exceeds six decimal places"
                );
            }

            for value in [start, step] {
                quantum = gcd(quantum, value);
            }

            name.push_str(&format!(
                "_{}_{}_{}",
                start.normalize(),
                end.normalize(),
                step.normalize()
            ));

            let mut value = if start.is_zero() { step } else { start };
            while value < end {
                ticks.push(Price::from_decimal_dp(value, PRICE_PRECISION)?);
                if step >= end - value {
                    break;
                }

                value += step;
            }

            previous_end = end;
        }

        Self::from_ticks(name, ticks, quantum)
    }

    fn from_ticks(name: String, ticks: Vec<Price>, quantum: Decimal) -> anyhow::Result<Self> {
        let scheme = TieredTickScheme::from_ticks(ticks, PRICE_PRECISION)?;
        let increment = Price::from_decimal_dp(quantum, PRICE_PRECISION)?;
        Ok(Self {
            name,
            scheme,
            increment,
        })
    }

    const fn increment(&self) -> Price {
        self.increment
    }

    fn complement(self) -> anyhow::Result<Self> {
        let name = format!("{}_NO", self.name);
        let ticks = self
            .scheme
            .ticks()
            .into_iter()
            .rev()
            .map(|price| Price::from_decimal_dp(Decimal::ONE - price.as_decimal(), PRICE_PRECISION))
            .collect::<Result<Vec<_>, _>>()?;
        let quantum = gcd(Decimal::ONE, self.increment.as_decimal());
        Self::from_ticks(name, ticks, quantum)
    }

    fn register(&self) -> anyhow::Result<Ustr> {
        let scheme = TickScheme::Tiered(self.scheme.clone());
        if let Err(e) = register_tick_scheme(&self.name, scheme.clone()) {
            anyhow::ensure!(
                get_tick_scheme(&self.name) == Some(scheme),
                "inconsistent registered price grid: {e}"
            );
        }

        Ok(Ustr::from(&self.name))
    }
}

fn gcd(mut a: Decimal, mut b: Decimal) -> Decimal {
    while !b.is_zero() {
        (a, b) = (b, a % b);
    }

    a
}
