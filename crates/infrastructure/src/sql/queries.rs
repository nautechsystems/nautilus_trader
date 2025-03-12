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

use std::collections::HashMap;

use nautilus_common::{custom::CustomData, signal::Signal};
use nautilus_model::{
    accounts::{Account, AccountAny},
    data::{Bar, DataType, QuoteTick, TradeTick},
    events::{
        AccountState, OrderEvent, OrderEventAny, OrderSnapshot,
        position::snapshot::PositionSnapshot,
    },
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, PositionId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{AccountBalance, Currency, MarginBalance},
};
use sqlx::{PgPool, Row};

use super::models::{
    orders::OrderSnapshotModel,
    positions::PositionSnapshotModel,
    types::{CustomDataModel, SignalModel},
};
use crate::sql::models::{
    accounts::AccountEventModel,
    data::{BarModel, QuoteTickModel, TradeTickModel},
    enums::{
        AggregationSourceModel, AggressorSideModel, AssetClassModel, BarAggregationModel,
        CurrencyTypeModel, PriceTypeModel, TrailingOffsetTypeModel,
    },
    general::{GeneralRow, OrderEventOrderClientIdCombination},
    instruments::InstrumentAnyModel,
    orders::OrderEventAnyModel,
    types::CurrencyModel,
};

#[derive(Debug)]
pub struct DatabaseQueries;

impl DatabaseQueries {
    pub async fn truncate(pool: &PgPool) -> anyhow::Result<()> {
        sqlx::query("SELECT truncate_all_tables()")
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to truncate tables: {e}"))
    }

    pub async fn add(pool: &PgPool, key: String, value: Vec<u8>) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO general (id, value) VALUES ($1, $2)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into general table: {e}"))
    }

    pub async fn load(pool: &PgPool) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        sqlx::query_as::<_, GeneralRow>("SELECT * FROM general")
            .fetch_all(pool)
            .await
            .map(|rows| {
                let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
                for row in rows {
                    cache.insert(row.id, row.value);
                }
                cache
            })
            .map_err(|e| anyhow::anyhow!("Failed to load general table: {e}"))
    }

    pub async fn add_currency(pool: &PgPool, currency: Currency) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO currency (id, precision, iso4217, name, currency_type) VALUES ($1, $2, $3, $4, $5::currency_type) ON CONFLICT (id) DO NOTHING"
        )
            .bind(currency.code.as_str())
            .bind(currency.precision as i32)
            .bind(currency.iso4217 as i32)
            .bind(currency.name.as_str())
            .bind(CurrencyTypeModel(currency.currency_type))
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into currency table: {e}"))
    }

    pub async fn load_currencies(pool: &PgPool) -> anyhow::Result<Vec<Currency>> {
        sqlx::query_as::<_, CurrencyModel>("SELECT * FROM currency ORDER BY id ASC")
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|row| row.0).collect())
            .map_err(|e| anyhow::anyhow!("Failed to load currencies: {e}"))
    }

    pub async fn load_currency(pool: &PgPool, code: &str) -> anyhow::Result<Option<Currency>> {
        sqlx::query_as::<_, CurrencyModel>("SELECT * FROM currency WHERE id = $1")
            .bind(code)
            .fetch_optional(pool)
            .await
            .map(|currency| currency.map(|row| row.0))
            .map_err(|e| anyhow::anyhow!("Failed to load currency: {e}"))
    }

    pub async fn add_instrument(
        pool: &PgPool,
        kind: &str,
        instrument: Box<dyn Instrument>,
    ) -> anyhow::Result<()> {
        sqlx::query(r#"
            INSERT INTO "instrument" (
                id, kind, raw_symbol, base_currency, underlying, quote_currency, settlement_currency, isin, asset_class, exchange,
                multiplier, option_kind, is_inverse, strike_price, activation_ns, expiration_ns, price_precision, size_precision,
                price_increment, size_increment, maker_fee, taker_fee, margin_init, margin_maint, lot_size, max_quantity, min_quantity, max_notional,
                min_notional, max_price, min_price, ts_init, ts_event, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::asset_class, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT (id)
            DO UPDATE
            SET
                kind = $2, raw_symbol = $3, base_currency= $4, underlying = $5, quote_currency = $6, settlement_currency = $7, isin = $8, asset_class = $9, exchange = $10,
                 multiplier = $11, option_kind = $12, is_inverse = $13, strike_price = $14, activation_ns = $15, expiration_ns = $16 , price_precision = $17, size_precision = $18,
                 price_increment = $19, size_increment = $20, maker_fee = $21, taker_fee = $22, margin_init = $23, margin_maint = $24, lot_size = $25, max_quantity = $26,
                 min_quantity = $27, max_notional = $28, min_notional = $29, max_price = $30, min_price = $31, ts_init = $32,  ts_event = $33, updated_at = CURRENT_TIMESTAMP
            "#)
            .bind(instrument.id().to_string())
            .bind(kind)
            .bind(instrument.raw_symbol().to_string())
            .bind(instrument.base_currency().map(|x| x.code.as_str()))
            .bind(instrument.underlying().map(|x| x.to_string()))
            .bind(instrument.quote_currency().code.as_str())
            .bind(instrument.settlement_currency().code.as_str())
            .bind(instrument.isin().map(|x| x.to_string()))
            .bind(AssetClassModel(instrument.asset_class()))
            .bind(instrument.exchange().map(|x| x.to_string()))
            .bind(instrument.multiplier().to_string())
            .bind(instrument.option_kind().map(|x| x.to_string()))
            .bind(instrument.is_inverse())
            .bind(instrument.strike_price().map(|x| x.to_string()))
            .bind(instrument.activation_ns().map(|x| x.to_string()))
            .bind(instrument.expiration_ns().map(|x| x.to_string()))
            .bind(instrument.price_precision() as i32)
            .bind(instrument.size_precision() as i32)
            .bind(instrument.price_increment().to_string())
            .bind(instrument.size_increment().to_string())
            .bind(instrument.maker_fee().to_string())
            .bind(instrument.taker_fee().to_string())
            .bind(instrument.margin_init().to_string())
            .bind(instrument.margin_maint().to_string())
            .bind(instrument.lot_size().map(|x| x.to_string()))
            .bind(instrument.max_quantity().map(|x| x.to_string()))
            .bind(instrument.min_quantity().map(|x| x.to_string()))
            .bind(instrument.max_notional().map(|x| x.to_string()))
            .bind(instrument.min_notional().map(|x| x.to_string()))
            .bind(instrument.max_price().map(|x| x.to_string()))
            .bind(instrument.min_price().map(|x| x.to_string()))
            .bind(instrument.ts_init().to_string())
            .bind(instrument.ts_event().to_string())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!(format!("Failed to insert item {} into instrument table: {:?}", instrument.id().to_string(), e)))
    }

    pub async fn load_instrument(
        pool: &PgPool,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        sqlx::query_as::<_, InstrumentAnyModel>("SELECT * FROM instrument WHERE id = $1")
            .bind(instrument_id.to_string())
            .fetch_optional(pool)
            .await
            .map(|instrument| instrument.map(|row| row.0))
            .map_err(|e| {
                anyhow::anyhow!("Failed to load instrument with id {instrument_id},error is: {e}")
            })
    }

    pub async fn load_instruments(pool: &PgPool) -> anyhow::Result<Vec<InstrumentAny>> {
        sqlx::query_as::<_, InstrumentAnyModel>("SELECT * FROM instrument")
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|row| row.0).collect())
            .map_err(|e| anyhow::anyhow!("Failed to load instruments: {e}"))
    }

    pub async fn add_order(
        pool: &PgPool,
        _kind: &str,
        updated: bool,
        order: Box<dyn Order>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        if updated {
            let exists =
                DatabaseQueries::check_if_order_initialized_exists(pool, order.client_order_id())
                    .await
                    .unwrap();
            if !exists {
                panic!(
                    "OrderInitialized event does not exist for order: {}",
                    order.client_order_id()
                );
            }
        }
        match order.last_event().clone() {
            OrderEventAny::Accepted(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::CancelRejected(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Canceled(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Denied(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Emulated(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Expired(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Filled(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Initialized(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::ModifyRejected(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::PendingCancel(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::PendingUpdate(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Rejected(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Released(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Submitted(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Updated(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
            OrderEventAny::Triggered(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
        }
    }

    pub async fn add_order_snapshot(pool: &PgPool, snapshot: OrderSnapshot) -> anyhow::Result<()> {
        let mut transaction = pool.begin().await?;

        // Insert trader if it does not exist
        // TODO remove this when node and trader initialization is implemented
        sqlx::query(
            r#"
            INSERT INTO "trader" (id) VALUES ($1) ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(snapshot.trader_id.to_string())
        .execute(&mut *transaction)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into trader table: {e}"))?;

        sqlx::query(
            r#"
            INSERT INTO "order" (
                id, trader_id, strategy_id, instrument_id, client_order_id, venue_order_id, position_id,
                account_id, last_trade_id, order_type, order_side, quantity, price, trigger_price,
                trigger_type, limit_offset, trailing_offset, trailing_offset_type, time_in_force,
                expire_time, filled_qty, liquidity_side, avg_px, slippage, commissions, status,
                is_post_only, is_reduce_only, is_quote_quantity, display_qty, emulation_trigger,
                trigger_instrument_id, contingency_type, order_list_id, linked_order_ids,
                parent_order_id, exec_algorithm_id, exec_algorithm_params, exec_spawn_id, tags, init_id, ts_init, ts_last,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $1, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                $17::TRAILING_OFFSET_TYPE, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28,
                $29, $30, $31, $32, $33, $34, $35, $36, $37, $38, $39, $40, $41, $42,
                CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE SET
                trader_id = $2,
                strategy_id = $3,
                instrument_id = $4,
                venue_order_id = $5,
                position_id = $6,
                account_id = $7,
                last_trade_id = $8,
                order_type = $9,
                order_side = $10,
                quantity = $11,
                price = $12,
                trigger_price = $13,
                trigger_type = $14,
                limit_offset = $15,
                trailing_offset = $16,
                trailing_offset_type = $17::TRAILING_OFFSET_TYPE,
                time_in_force = $18,
                expire_time = $19,
                filled_qty = $20,
                liquidity_side = $21,
                avg_px = $22,
                slippage = $23,
                commissions = $24,
                status = $25,
                is_post_only = $26,
                is_reduce_only = $27,
                is_quote_quantity = $28,
                display_qty = $29,
                emulation_trigger = $30,
                trigger_instrument_id = $31,
                contingency_type = $32,
                order_list_id = $33,
                linked_order_ids = $34,
                parent_order_id = $35,
                exec_algorithm_id = $36,
                exec_algorithm_params = $37,
                exec_spawn_id = $38,
                tags = $39,
                init_id = $40,
                ts_init = $41,
                ts_last = $42,
                updated_at = CURRENT_TIMESTAMP
        "#)
            .bind(snapshot.client_order_id.to_string())  // Used for both id and client_order_id
            .bind(snapshot.trader_id.to_string())
            .bind(snapshot.strategy_id.to_string())
            .bind(snapshot.instrument_id.to_string())
            .bind(snapshot.venue_order_id.map(|x| x.to_string()))
            .bind(snapshot.position_id.map(|x| x.to_string()))
            .bind(snapshot.account_id.map(|x| x.to_string()))
            .bind(snapshot.last_trade_id.map(|x| x.to_string()))
            .bind(snapshot.order_type.to_string())
            .bind(snapshot.order_side.to_string())
            .bind(snapshot.quantity.to_string())
            .bind(snapshot.price.map(|x| x.to_string()))
            .bind(snapshot.trigger_price.map(|x| x.to_string()))
            .bind(snapshot.trigger_type.map(|x| x.to_string()))
            .bind(snapshot.limit_offset.map(|x| x.to_string()))
            .bind(snapshot.trailing_offset.map(|x| x.to_string()))
            .bind(snapshot.trailing_offset_type.map(|x| x.to_string()))
            .bind(snapshot.time_in_force.to_string())
            .bind(snapshot.expire_time.map(|x| x.to_string()))
            .bind(snapshot.filled_qty.to_string())
            .bind(snapshot.liquidity_side.map(|x| x.to_string()))
            .bind(snapshot.avg_px)
            .bind(snapshot.slippage)
            .bind(snapshot.commissions.iter().map(|x| x.to_string()).collect::<Vec<String>>())
            .bind(snapshot.status.to_string())
            .bind(snapshot.is_post_only)
            .bind(snapshot.is_reduce_only)
            .bind(snapshot.is_quote_quantity)
            .bind(snapshot.display_qty.map(|x| x.to_string()))
            .bind(snapshot.emulation_trigger.map(|x| x.to_string()))
            .bind(snapshot.trigger_instrument_id.map(|x| x.to_string()))
            .bind(snapshot.contingency_type.map(|x| x.to_string()))
            .bind(snapshot.order_list_id.map(|x| x.to_string()))
            .bind(snapshot.linked_order_ids.map(|x| x.iter().map(|x| x.to_string()).collect::<Vec<String>>()))
            .bind(snapshot.parent_order_id.map(|x| x.to_string()))
            .bind(snapshot.exec_algorithm_id.map(|x| x.to_string()))
            .bind(snapshot.exec_algorithm_params.map(|x| serde_json::to_value(x).unwrap()))
            .bind(snapshot.exec_spawn_id.map(|x| x.to_string()))
            .bind(snapshot.tags.map(|x| x.iter().map(|x| x.to_string()).collect::<Vec<String>>()))
            .bind(snapshot.init_id.to_string())
            .bind(snapshot.ts_init.to_string())
            .bind(snapshot.ts_last.to_string())
            .execute(&mut *transaction)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into order table: {e}"))?;

        transaction
            .commit()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to commit transaction: {e}"))
    }

    pub async fn load_order_snapshot(
        pool: &PgPool,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderSnapshot>> {
        sqlx::query_as::<_, OrderSnapshotModel>(
            r#"SELECT * FROM "order" WHERE client_order_id = $1"#,
        )
        .bind(client_order_id.to_string())
        .fetch_optional(pool)
        .await
        .map(|model| model.map(|m| m.0))
        .map_err(|e| anyhow::anyhow!("Failed to load order snapshot: {e}"))
    }

    pub async fn add_position_snapshot(
        pool: &PgPool,
        snapshot: PositionSnapshot,
    ) -> anyhow::Result<()> {
        let mut transaction = pool.begin().await?;

        // Insert trader if it does not exist
        // TODO remove this when node and trader initialization is implemented
        sqlx::query(
            r#"
            INSERT INTO "trader" (id) VALUES ($1) ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(snapshot.trader_id.to_string())
        .execute(&mut *transaction)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into trader table: {e}"))?;

        sqlx::query(r#"
            INSERT INTO "position" (
                id, trader_id, strategy_id, instrument_id, account_id, opening_order_id, closing_order_id, entry, side, signed_qty, quantity, peak_qty,
                quote_currency, base_currency, settlement_currency, avg_px_open, avg_px_close, realized_return, realized_pnl, unrealized_pnl, commissions,
                duration_ns, ts_opened, ts_closed, ts_init, ts_last, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20,
                $21, $22, $23, $24, $25, $26, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                trader_id = $2, strategy_id = $3, instrument_id = $4, account_id = $5, opening_order_id = $6, closing_order_id = $7, entry = $8, side = $9, signed_qty = $10, quantity = $11,
                peak_qty = $12, quote_currency = $13, base_currency = $14, settlement_currency = $15, avg_px_open = $16, avg_px_close = $17, realized_return = $18, realized_pnl = $19, unrealized_pnl = $20,
                commissions = $21, duration_ns = $22, ts_opened = $23, ts_closed = $24, ts_init = $25, ts_last = $26, updated_at = CURRENT_TIMESTAMP
        "#)
            .bind(snapshot.position_id.to_string())
            .bind(snapshot.trader_id.to_string())
            .bind(snapshot.strategy_id.to_string())
            .bind(snapshot.instrument_id.to_string())
            .bind(snapshot.account_id.to_string())
            .bind(snapshot.opening_order_id.to_string())
            .bind(snapshot.closing_order_id.map(|x| x.to_string()))
            .bind(snapshot.entry.to_string())
            .bind(snapshot.side.to_string())
            .bind(snapshot.signed_qty)
            .bind(snapshot.quantity.to_string())
            .bind(snapshot.peak_qty.to_string())
            .bind(snapshot.quote_currency.to_string())
            .bind(snapshot.base_currency.map(|x| x.to_string()))
            .bind(snapshot.settlement_currency.to_string())
            .bind(snapshot.avg_px_open)
            .bind(snapshot.avg_px_close)
            .bind(snapshot.realized_return)
            .bind(snapshot.realized_pnl.map(|x| x.to_string()))
            .bind(snapshot.unrealized_pnl.map(|x| x.to_string()))
            .bind(snapshot.commissions.iter().map(|x| x.to_string()).collect::<Vec<String>>())
            .bind(snapshot.duration_ns.map(|x| x.to_string()))
            .bind(snapshot.ts_opened.to_string())
            .bind(snapshot.ts_closed.map(|x| x.to_string()))
            .bind(snapshot.ts_init.to_string())
            .bind(snapshot.ts_last.to_string())
            .execute(&mut *transaction)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into position table: {e}"))?;
        transaction
            .commit()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to commit transaction: {e}"))
    }

    pub async fn load_position_snapshot(
        pool: &PgPool,
        position_id: &PositionId,
    ) -> anyhow::Result<Option<PositionSnapshot>> {
        sqlx::query_as::<_, PositionSnapshotModel>(r#"SELECT * FROM "position" WHERE id = $1"#)
            .bind(position_id.to_string())
            .fetch_optional(pool)
            .await
            .map(|model| model.map(|m| m.0))
            .map_err(|e| anyhow::anyhow!("Failed to load position snapshot: {e}"))
    }

    pub async fn check_if_order_initialized_exists(
        pool: &PgPool,
        client_order_id: ClientOrderId,
    ) -> anyhow::Result<bool> {
        sqlx::query(r#"
            SELECT EXISTS(SELECT 1 FROM "order_event" WHERE client_order_id = $1 AND kind = 'OrderInitialized')
        "#)
            .bind(client_order_id.to_string())
            .fetch_one(pool)
            .await
            .map(|row| row.get(0))
            .map_err(|e| anyhow::anyhow!("Failed to check if order initialized exists: {e}"))
    }

    pub async fn check_if_account_event_exists(
        pool: &PgPool,
        account_id: AccountId,
    ) -> anyhow::Result<bool> {
        sqlx::query(
            r#"
            SELECT EXISTS(SELECT 1 FROM "account_event" WHERE account_id = $1)
        "#,
        )
        .bind(account_id.to_string())
        .fetch_one(pool)
        .await
        .map(|row| row.get(0))
        .map_err(|e| anyhow::anyhow!("Failed to check if account event exists: {e}"))
    }

    pub async fn add_order_event(
        pool: &PgPool,
        order_event: Box<dyn OrderEvent>,
        client_id: Option<ClientId>,
    ) -> anyhow::Result<()> {
        let mut transaction = pool.begin().await?;

        // Insert trader if it does not exist
        // TODO remove this when node and trader initialization is implemented
        sqlx::query(
            r#"
            INSERT INTO "trader" (id) VALUES ($1) ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(order_event.trader_id().to_string())
        .execute(&mut *transaction)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into trader table: {e}"))?;

        // Insert client if it does not exist
        // TODO remove this when client initialization is implemented
        if let Some(client_id) = client_id {
            sqlx::query(
                r#"
                INSERT INTO "client" (id) VALUES ($1) ON CONFLICT (id) DO NOTHING
            "#,
            )
            .bind(client_id.to_string())
            .execute(&mut *transaction)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into client table: {e}"))?;
        }

        sqlx::query(r#"
            INSERT INTO "order_event" (
                id, kind, client_order_id, order_type, order_side, trader_id, client_id, strategy_id, instrument_id, trade_id, currency, quantity, time_in_force, liquidity_side,
                post_only, reduce_only, quote_quantity, reconciliation, price, last_px, last_qty, trigger_price, trigger_type, limit_offset, trailing_offset,
                trailing_offset_type, expire_time, display_qty, emulation_trigger, trigger_instrument_id, contingency_type,
                order_list_id, linked_order_ids, parent_order_id,
                exec_algorithm_id, exec_spawn_id, venue_order_id, account_id, position_id, commission, ts_event, ts_init, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20,
                $21, $22, $23, $24, $25::trailing_offset_type, $26, $27, $28, $29, $30, $31, $32, $33, $34,
                $35, $36, $37, $38, $39, $40, $41, $42,  CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                kind = $2, client_order_id = $3, order_type = $4, order_side=$5, trader_id = $6, client_id = $7, strategy_id = $8, instrument_id = $9, trade_id = $10, currency = $11,
                quantity = $12, time_in_force = $13, liquidity_side = $14, post_only = $15, reduce_only = $16, quote_quantity = $17, reconciliation = $18, price = $19, last_px = $20,
                last_qty = $21, trigger_price = $22, trigger_type = $23, limit_offset = $24, trailing_offset = $25, trailing_offset_type = $26, expire_time = $27, display_qty = $28,
                emulation_trigger = $29, trigger_instrument_id = $30, contingency_type = $31, order_list_id = $32, linked_order_ids = $33, parent_order_id = $34, exec_algorithm_id = $35,
                exec_spawn_id = $36, venue_order_id = $37, account_id = $38, position_id = $39, commission = $40, ts_event = $41, ts_init = $42, updated_at = CURRENT_TIMESTAMP

        "#)
            .bind(order_event.id().to_string())
            .bind(order_event.kind())
            .bind(order_event.client_order_id().to_string())
            .bind(order_event.order_type().map(|x| x.to_string()))
            .bind(order_event.order_side().map(|x| x.to_string()))
            .bind(order_event.trader_id().to_string())
            .bind(client_id.map(|x| x.to_string()))
            .bind(order_event.strategy_id().to_string())
            .bind(order_event.instrument_id().to_string())
            .bind(order_event.trade_id().map(|x| x.to_string()))
            .bind(order_event.currency().map(|x| x.code.as_str()))
            .bind(order_event.quantity().map(|x| x.to_string()))
            .bind(order_event.time_in_force().map(|x| x.to_string()))
            .bind(order_event.liquidity_side().map(|x| x.to_string()))
            .bind(order_event.post_only())
            .bind(order_event.reduce_only())
            .bind(order_event.quote_quantity())
            .bind(order_event.reconciliation())
            .bind(order_event.price().map(|x| x.to_string()))
            .bind(order_event.last_px().map(|x| x.to_string()))
            .bind(order_event.last_qty().map(|x| x.to_string()))
            .bind(order_event.trigger_price().map(|x| x.to_string()))
            .bind(order_event.trigger_type().map(|x| x.to_string()))
            .bind(order_event.limit_offset().map(|x| x.to_string()))
            .bind(order_event.trailing_offset().map(|x| x.to_string()))
            .bind(order_event.trailing_offset_type().map(TrailingOffsetTypeModel))
            .bind(order_event.expire_time().map(|x| x.to_string()))
            .bind(order_event.display_qty().map(|x| x.to_string()))
            .bind(order_event.emulation_trigger().map(|x| x.to_string()))
            .bind(order_event.trigger_instrument_id().map(|x| x.to_string()))
            .bind(order_event.contingency_type().map(|x| x.to_string()))
            .bind(order_event.order_list_id().map(|x| x.to_string()))
            .bind(order_event.linked_order_ids().map(|x| x.iter().map(|x| x.to_string()).collect::<Vec<String>>()))
            .bind(order_event.parent_order_id().map(|x| x.to_string()))
            .bind(order_event.exec_algorithm_id().map(|x| x.to_string()))
            .bind(order_event.exec_spawn_id().map(|x| x.to_string()))
            .bind(order_event.venue_order_id().map(|x| x.to_string()))
            .bind(order_event.account_id().map(|x| x.to_string()))
            .bind(order_event.position_id().map(|x| x.to_string()))
            .bind(order_event.commission().map(|x| x.to_string()))
            .bind(order_event.ts_event().to_string())
            .bind(order_event.ts_init().to_string())
            .execute(&mut *transaction)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into order_event table: {e}"))?;
        transaction
            .commit()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to commit transaction: {e}"))
    }

    pub async fn load_order_events(
        pool: &PgPool,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Vec<OrderEventAny>> {
        sqlx::query_as::<_, OrderEventAnyModel>(r#"SELECT * FROM "order_event" event WHERE event.client_order_id = $1 ORDER BY created_at ASC"#)
        .bind(client_order_id.to_string())
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|e| anyhow::anyhow!("Failed to load order events: {e}"))
    }

    pub async fn load_order(
        pool: &PgPool,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        let order_events = DatabaseQueries::load_order_events(pool, client_order_id).await;

        match order_events {
            Ok(order_events) => {
                if order_events.is_empty() {
                    return Ok(None);
                }
                let order = OrderAny::from_events(order_events).unwrap();
                Ok(Some(order))
            }
            Err(e) => anyhow::bail!("Failed to load order events: {e}"),
        }
    }

    pub async fn load_orders(pool: &PgPool) -> anyhow::Result<Vec<OrderAny>> {
        let mut orders: Vec<OrderAny> = Vec::new();
        let client_order_ids: Vec<ClientOrderId> = sqlx::query(
            r#"
            SELECT DISTINCT client_order_id FROM "order_event"
        "#,
        )
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| ClientOrderId::from(row.get::<&str, _>(0)))
                .collect()
        })
        .map_err(|e| anyhow::anyhow!("Failed to load order ids: {e}"))?;
        for id in client_order_ids {
            let order = DatabaseQueries::load_order(pool, &id).await.unwrap();
            match order {
                Some(order) => {
                    orders.push(order);
                }
                None => {
                    continue;
                }
            }
        }
        Ok(orders)
    }

    pub async fn add_account(
        pool: &PgPool,
        kind: &str,
        updated: bool,
        account: Box<dyn Account>,
    ) -> anyhow::Result<()> {
        if updated {
            let exists = DatabaseQueries::check_if_account_event_exists(pool, account.id())
                .await
                .unwrap();
            if !exists {
                panic!("Account event does not exist for account: {}", account.id());
            }
        }

        let mut transaction = pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO "account" (id) VALUES ($1) ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(account.id().to_string())
        .execute(&mut *transaction)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into account table: {e}"))?;

        let account_event = account.last_event().unwrap();
        sqlx::query(r#"
            INSERT INTO "account_event" (
                id, kind, account_id, base_currency, balances, margins, is_reported, ts_event, ts_init, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                kind = $2, account_id = $3, base_currency = $4, balances = $5, margins = $6, is_reported = $7,
                ts_event = $8, ts_init = $9, updated_at = CURRENT_TIMESTAMP
        "#)
            .bind(account_event.event_id.to_string())
            .bind(kind.to_string())
            .bind(account_event.account_id.to_string())
            .bind(account_event.base_currency.map(|x| x.code.as_str()))
            .bind(serde_json::to_value::<Vec<AccountBalance>>(account_event.balances).unwrap())
            .bind(serde_json::to_value::<Vec<MarginBalance>>(account_event.margins).unwrap())
            .bind(account_event.is_reported)
            .bind(account_event.ts_event.to_string())
            .bind(account_event.ts_init.to_string())
            .execute(&mut *transaction)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into account_event table: {e}"))?;
        transaction
            .commit()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to commit add_account transaction: {e}"))
    }

    pub async fn load_account_events(
        pool: &PgPool,
        account_id: &AccountId,
    ) -> anyhow::Result<Vec<AccountState>> {
        sqlx::query_as::<_, AccountEventModel>(
            r#"SELECT * FROM "account_event" WHERE account_id = $1 ORDER BY created_at ASC"#,
        )
        .bind(account_id.to_string())
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|e| anyhow::anyhow!("Failed to load account events: {e}"))
    }

    pub async fn load_account(
        pool: &PgPool,
        account_id: &AccountId,
    ) -> anyhow::Result<Option<AccountAny>> {
        let account_events = DatabaseQueries::load_account_events(pool, account_id).await;
        match account_events {
            Ok(account_events) => {
                if account_events.is_empty() {
                    return Ok(None);
                }
                let account = AccountAny::from_events(account_events).unwrap();
                Ok(Some(account))
            }
            Err(e) => anyhow::bail!("Failed to load account events: {e}"),
        }
    }

    pub async fn load_accounts(pool: &PgPool) -> anyhow::Result<Vec<AccountAny>> {
        let mut accounts: Vec<AccountAny> = Vec::new();
        let account_ids: Vec<AccountId> = sqlx::query(
            r#"
            SELECT DISTINCT account_id FROM "account_event"
        "#,
        )
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| AccountId::from(row.get::<&str, _>(0)))
                .collect()
        })
        .map_err(|e| anyhow::anyhow!("Failed to load account ids: {e}"))?;
        for id in account_ids {
            let account = DatabaseQueries::load_account(pool, &id).await.unwrap();
            match account {
                Some(account) => {
                    accounts.push(account);
                }
                None => {
                    continue;
                }
            }
        }
        Ok(accounts)
    }

    pub async fn add_trade(pool: &PgPool, trade: &TradeTick) -> anyhow::Result<()> {
        sqlx::query(r#"
            INSERT INTO "trade" (
                instrument_id, price, quantity, aggressor_side, venue_trade_id,
                ts_event, ts_init, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4::aggressor_side, $5, $6, $7, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                instrument_id = $1, price = $2, quantity = $3, aggressor_side = $4, venue_trade_id = $5,
                ts_event = $6, ts_init = $7, updated_at = CURRENT_TIMESTAMP
        "#)
            .bind(trade.instrument_id.to_string())
            .bind(trade.price.to_string())
            .bind(trade.size.to_string())
            .bind(AggressorSideModel(trade.aggressor_side))
            .bind(trade.trade_id.to_string())
            .bind(trade.ts_event.to_string())
            .bind(trade.ts_init.to_string())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into trade table: {e}"))
    }

    pub async fn load_trades(
        pool: &PgPool,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Vec<TradeTick>> {
        sqlx::query_as::<_, TradeTickModel>(
            r#"SELECT * FROM "trade" WHERE instrument_id = $1 ORDER BY ts_event ASC"#,
        )
        .bind(instrument_id.to_string())
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|e| anyhow::anyhow!("Failed to load trades: {e}"))
    }

    pub async fn add_quote(pool: &PgPool, quote: &QuoteTick) -> anyhow::Result<()> {
        sqlx::query(r#"
            INSERT INTO "quote" (
                instrument_id, bid_price, ask_price, bid_size, ask_size, ts_event, ts_init, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                instrument_id = $1, bid_price = $2, ask_price = $3, bid_size = $4, ask_size = $5,
                ts_event = $6, ts_init = $7, updated_at = CURRENT_TIMESTAMP
        "#)
            .bind(quote.instrument_id.to_string())
            .bind(quote.bid_price.to_string())
            .bind(quote.ask_price.to_string())
            .bind(quote.bid_size.to_string())
            .bind(quote.ask_size.to_string())
            .bind(quote.ts_event.to_string())
            .bind(quote.ts_init.to_string())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into quote table: {e}"))
    }

    pub async fn load_quotes(
        pool: &PgPool,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        sqlx::query_as::<_, QuoteTickModel>(
            r#"SELECT * FROM "quote" WHERE instrument_id = $1 ORDER BY ts_event ASC"#,
        )
        .bind(instrument_id.to_string())
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|e| anyhow::anyhow!("Failed to load quotes: {e}"))
    }

    pub async fn add_bar(pool: &PgPool, bar: &Bar) -> anyhow::Result<()> {
        println!("Adding bar: {:?}", bar);
        sqlx::query(r#"
            INSERT INTO "bar" (
                instrument_id, step, bar_aggregation, price_type, aggregation_source, open, high, low, close, volume, ts_event, ts_init, created_at, updated_at
            ) VALUES (
                $1, $2, $3::bar_aggregation, $4::price_type, $5::aggregation_source, $6, $7, $8, $9, $10, $11, $12, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                instrument_id = $1, step = $2, bar_aggregation = $3::bar_aggregation, price_type = $4::price_type, aggregation_source = $5::aggregation_source,
                open = $6, high = $7, low = $8, close = $9, volume = $10, ts_event = $11, ts_init = $12, updated_at = CURRENT_TIMESTAMP
        "#)
            .bind(bar.bar_type.instrument_id().to_string())
            .bind(bar.bar_type.spec().step.get() as i32)
            .bind(BarAggregationModel(bar.bar_type.spec().aggregation))
            .bind(PriceTypeModel(bar.bar_type.spec().price_type))
            .bind(AggregationSourceModel(bar.bar_type.aggregation_source()))
            .bind(bar.open.to_string())
            .bind(bar.high.to_string())
            .bind(bar.low.to_string())
            .bind(bar.close.to_string())
            .bind(bar.volume.to_string())
            .bind(bar.ts_event.to_string())
            .bind(bar.ts_init.to_string())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to insert into bar table: {e}"))
    }

    pub async fn load_bars(
        pool: &PgPool,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Vec<Bar>> {
        sqlx::query_as::<_, BarModel>(
            r#"SELECT * FROM "bar" WHERE instrument_id = $1 ORDER BY ts_event ASC"#,
        )
        .bind(instrument_id.to_string())
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|e| anyhow::anyhow!("Failed to load bars: {e}"))
    }

    pub async fn load_distinct_order_event_client_ids(
        pool: &PgPool,
    ) -> anyhow::Result<HashMap<ClientOrderId, ClientId>> {
        let mut map: HashMap<ClientOrderId, ClientId> = HashMap::new();
        let result = sqlx::query_as::<_, OrderEventOrderClientIdCombination>(
            r#"
            SELECT DISTINCT
                client_order_id AS "client_order_id",
                client_id AS "client_id"
            FROM "order_event"
        "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load account ids: {e}"))?;
        for id in result {
            map.insert(id.client_order_id, id.client_id);
        }
        Ok(map)
    }

    pub async fn add_signal(pool: &PgPool, signal: &Signal) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO "signal" (
                name, value, ts_event, ts_init, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                name = $1, value = $2, ts_event = $3, ts_init = $4,
                updated_at = CURRENT_TIMESTAMP
        "#,
        )
        .bind(signal.name.to_string())
        .bind(signal.value.to_string())
        .bind(signal.ts_event.to_string())
        .bind(signal.ts_init.to_string())
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into signal table: {e}"))
    }

    pub async fn load_signals(pool: &PgPool, name: &str) -> anyhow::Result<Vec<Signal>> {
        sqlx::query_as::<_, SignalModel>(
            r#"SELECT * FROM "signal" WHERE name = $1 ORDER BY ts_init ASC"#,
        )
        .bind(name)
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|e| anyhow::anyhow!("Failed to load signals: {e}"))
    }

    pub async fn add_custom_data(pool: &PgPool, data: &CustomData) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO "custom" (
                data_type, metadata, value, ts_event, ts_init, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            ON CONFLICT (id)
            DO UPDATE
            SET
                data_type = $1, metadata = $2, value = $3, ts_event = $4, ts_init = $5,
                updated_at = CURRENT_TIMESTAMP
        "#,
        )
        .bind(data.data_type.type_name().to_string())
        .bind(
            data.data_type
                .metadata()
                .as_ref()
                .map_or_else(|| Ok(serde_json::Value::Null), serde_json::to_value)?,
        )
        .bind(data.value.to_vec())
        .bind(data.ts_event.to_string())
        .bind(data.ts_init.to_string())
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into custom table: {e}"))
    }

    pub async fn load_custom_data(
        pool: &PgPool,
        data_type: &DataType,
    ) -> anyhow::Result<Vec<CustomData>> {
        // TODO: This metadata JSON could be more efficient at some point
        let metadata_json = data_type
            .metadata()
            .as_ref()
            .map_or(Ok(serde_json::Value::Null), |metadata| {
                serde_json::to_value(metadata)
            })?;

        sqlx::query_as::<_, CustomDataModel>(
            r#"SELECT * FROM "custom" WHERE data_type = $1 AND metadata = $2 ORDER BY ts_init ASC"#,
        )
        .bind(data_type.type_name())
        .bind(metadata_json)
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|e| anyhow::anyhow!("Failed to load custom data: {e}"))
    }
}
