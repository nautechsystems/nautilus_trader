// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{
        bar::{Bar, BarSpecification, BarType},
        quote::QuoteTick,
        trade::TradeTick,
    },
    identifiers::{InstrumentId, TradeId},
    types::{price::Price, quantity::Quantity},
};
use sqlx::{postgres::PgRow, Error, FromRow, Row};

use crate::sql::models::enums::{
    AggregationSourceModel, AggressorSideModel, BarAggregationModel, PriceTypeModel,
};

pub struct TradeTickModel(pub TradeTick);
pub struct QuoteTickModel(pub QuoteTick);
pub struct BarModel(pub Bar);

impl<'r> FromRow<'r, PgRow> for TradeTickModel {
    fn from_row(row: &'r PgRow) -> Result<Self, Error> {
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(|x| InstrumentId::from_str(x).unwrap())?;
        let price = row
            .try_get::<&str, _>("price")
            .map(|x| Price::from_str(x).unwrap())?;
        let size = row
            .try_get::<&str, _>("quantity")
            .map(|x| Quantity::from_str(x).unwrap())?;
        let aggressor_side = row
            .try_get::<AggressorSideModel, _>("aggressor_side")
            .map(|x| x.0)?;
        let trade_id = row
            .try_get::<&str, _>("venue_trade_id")
            .map(TradeId::from)?;
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let trade = TradeTick::new(
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        );
        Ok(TradeTickModel(trade))
    }
}

impl<'r> FromRow<'r, PgRow> for QuoteTickModel {
    fn from_row(row: &'r PgRow) -> Result<Self, Error> {
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(|x| InstrumentId::from_str(x).unwrap())?;
        let bid_price = row
            .try_get::<&str, _>("bid_price")
            .map(|x| Price::from_str(x).unwrap())?;
        let ask_price = row
            .try_get::<&str, _>("ask_price")
            .map(|x| Price::from_str(x).unwrap())?;
        let bid_size = row
            .try_get::<&str, _>("bid_size")
            .map(|x| Quantity::from_str(x).unwrap())?;
        let ask_size = row
            .try_get::<&str, _>("ask_size")
            .map(|x| Quantity::from_str(x).unwrap())?;
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let quote = QuoteTick::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        );
        Ok(QuoteTickModel(quote))
    }
}

impl<'r> FromRow<'r, PgRow> for BarModel {
    fn from_row(row: &'r PgRow) -> Result<Self, Error> {
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(|x| InstrumentId::from_str(x).unwrap())?;
        let step = row.try_get::<i32, _>("step")?;
        let price_type = row
            .try_get::<PriceTypeModel, _>("price_type")
            .map(|x| x.0)?;
        let bar_aggregation = row
            .try_get::<BarAggregationModel, _>("bar_aggregation")
            .map(|x| x.0)?;
        let aggregation_source = row
            .try_get::<AggregationSourceModel, _>("aggregation_source")
            .map(|x| x.0)?;
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(step as usize, bar_aggregation, price_type),
            aggregation_source,
        );
        let open = row
            .try_get::<&str, _>("open")
            .map(|x| Price::from_str(x).unwrap())?;
        let high = row
            .try_get::<&str, _>("high")
            .map(|x| Price::from_str(x).unwrap())?;
        let low = row
            .try_get::<&str, _>("low")
            .map(|x| Price::from_str(x).unwrap())?;
        let close = row
            .try_get::<&str, _>("close")
            .map(|x| Price::from_str(x).unwrap())?;
        let volume = row
            .try_get::<&str, _>("volume")
            .map(|x| Quantity::from_str(x).unwrap())?;
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let bar = Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init);
        Ok(BarModel(bar))
    }
}
