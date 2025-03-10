// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, PositionSide},
    events::PositionSnapshot,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    types::{Currency, Money, Quantity},
};
use sqlx::{FromRow, Row, postgres::PgRow};

pub struct PositionSnapshotModel(pub PositionSnapshot);

impl<'r> FromRow<'r, PgRow> for PositionSnapshotModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<&str, _>("id").map(PositionId::from)?;
        let trader_id = row.try_get::<&str, _>("trader_id").map(TraderId::from)?;
        let strategy_id = row
            .try_get::<&str, _>("strategy_id")
            .map(StrategyId::from)?;
        let instrument_id = row
            .try_get::<&str, _>("instrument_id")
            .map(InstrumentId::from)?;
        let account_id = row.try_get::<&str, _>("account_id").map(AccountId::from)?;
        let opening_order_id = row
            .try_get::<&str, _>("opening_order_id")
            .map(ClientOrderId::from)?;
        let closing_order_id = row
            .try_get::<Option<&str>, _>("closing_order_id")
            .ok()
            .and_then(|x| x.map(ClientOrderId::from));
        let entry = row
            .try_get::<&str, _>("entry")
            .map(OrderSide::from_str)?
            .expect("Invalid `OrderSide`");
        let side = row
            .try_get::<&str, _>("side")
            .map(PositionSide::from_str)?
            .expect("Invalid `PositionSide`");
        let signed_qty = row.try_get::<f64, _>("signed_qty")?;
        let quantity = row.try_get::<&str, _>("quantity").map(Quantity::from)?;
        let peak_qty = row.try_get::<&str, _>("peak_qty").map(Quantity::from)?;
        let quote_currency = row
            .try_get::<&str, _>("quote_currency")
            .map(Currency::from)?;
        let base_currency = row
            .try_get::<Option<&str>, _>("base_currency")
            .ok()
            .and_then(|x| x.map(Currency::from));
        let settlement_currency = row
            .try_get::<&str, _>("settlement_currency")
            .map(Currency::from)?;
        let avg_px_open = row.try_get::<f64, _>("avg_px_open")?;
        let avg_px_close = row.try_get::<Option<f64>, _>("avg_px_close")?;
        let realized_return = row.try_get::<Option<f64>, _>("realized_return")?;
        let realized_pnl = row.try_get::<&str, _>("realized_pnl").map(Money::from)?;
        let unrealized_pnl = row
            .try_get::<Option<&str>, _>("unrealized_pnl")
            .ok()
            .and_then(|x| x.map(Money::from));
        let commissions = row
            .try_get::<Option<Vec<String>>, _>("commissions")?
            .map_or_else(Vec::new, |c| {
                c.into_iter().map(|s| Money::from(&s)).collect()
            });
        let duration_ns: Option<u64> = row
            .try_get::<Option<i64>, _>("duration_ns")?
            .map(|value| value as u64);
        let ts_opened = row.try_get::<String, _>("ts_opened").map(UnixNanos::from)?;
        let ts_closed: Option<UnixNanos> = row
            .try_get::<Option<String>, _>("ts_closed")?
            .map(UnixNanos::from);
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;
        let ts_last = row.try_get::<String, _>("ts_last").map(UnixNanos::from)?;

        let snapshot = PositionSnapshot {
            trader_id,
            strategy_id,
            instrument_id,
            position_id: id,
            account_id,
            opening_order_id,
            closing_order_id,
            entry,
            side,
            signed_qty,
            quantity,
            peak_qty,
            quote_currency,
            base_currency,
            settlement_currency,
            avg_px_open,
            avg_px_close,
            realized_return,
            realized_pnl: Some(realized_pnl), // TODO: Standardize
            unrealized_pnl,
            commissions,
            duration_ns,
            ts_opened,
            ts_closed,
            ts_last,
            ts_init,
        };

        Ok(PositionSnapshotModel(snapshot))
    }
}
