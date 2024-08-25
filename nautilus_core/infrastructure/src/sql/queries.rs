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

use std::collections::HashMap;

use nautilus_model::{
    accounts::{any::AccountAny, base::Account},
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    events::{
        account::state::AccountState,
        order::{OrderEvent, OrderEventAny},
    },
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId},
    instruments::{any::InstrumentAny, Instrument},
    orders::{any::OrderAny, base::Order},
    types::{
        balance::{AccountBalance, MarginBalance},
        currency::Currency,
    },
};
use sqlx::{PgPool, Row};

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

pub struct DatabaseQueries;

impl DatabaseQueries {
    pub async fn add(pool: &PgPool, key: String, value: Vec<u8>) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO general (id, value) VALUES ($1, $2)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!("Failed to insert into general table: {err}"))
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
            .map_err(|err| anyhow::anyhow!("Failed to load general table: {err}"))
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
            .map_err(|err| anyhow::anyhow!("Failed to insert into currency table: {err}"))
    }

    pub async fn load_currencies(pool: &PgPool) -> anyhow::Result<Vec<Currency>> {
        sqlx::query_as::<_, CurrencyModel>("SELECT * FROM currency ORDER BY id ASC")
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|row| row.0).collect())
            .map_err(|err| anyhow::anyhow!("Failed to load currencies: {err}"))
    }

    pub async fn load_currency(pool: &PgPool, code: &str) -> anyhow::Result<Option<Currency>> {
        sqlx::query_as::<_, CurrencyModel>("SELECT * FROM currency WHERE id = $1")
            .bind(code)
            .fetch_optional(pool)
            .await
            .map(|currency| currency.map(|row| row.0))
            .map_err(|err| anyhow::anyhow!("Failed to load currency: {err}"))
    }

    pub async fn truncate(pool: &PgPool, table: String) -> anyhow::Result<()> {
        sqlx::query(format!("TRUNCATE TABLE {} CASCADE", table).as_str())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!("Failed to truncate table: {err}"))
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
            .map_err(|err| anyhow::anyhow!(format!("Failed to insert item {} into instrument table: {:?}", instrument.id().to_string(), err)))
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
            .map_err(|err| {
                anyhow::anyhow!("Failed to load instrument with id {instrument_id},error is: {err}")
            })
    }

    pub async fn load_instruments(pool: &PgPool) -> anyhow::Result<Vec<InstrumentAny>> {
        sqlx::query_as::<_, InstrumentAnyModel>("SELECT * FROM instrument")
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|row| row.0).collect())
            .map_err(|err| anyhow::anyhow!("Failed to load instruments: {err}"))
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
            OrderEventAny::PartiallyFilled(event) => {
                DatabaseQueries::add_order_event(pool, Box::new(event), client_id).await
            }
        }
    }

    pub async fn check_if_order_initialized_exists(
        pool: &PgPool,
        order_id: ClientOrderId,
    ) -> anyhow::Result<bool> {
        sqlx::query(r#"
            SELECT EXISTS(SELECT 1 FROM "order_event" WHERE order_id = $1 AND kind = 'OrderInitialized')
        "#)
            .bind(order_id.to_string())
            .fetch_one(pool)
            .await
            .map(|row| row.get(0))
            .map_err(|err| anyhow::anyhow!("Failed to check if order initialized exists: {err}"))
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
        .map_err(|err| anyhow::anyhow!("Failed to check if account event exists: {err}"))
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
        .map_err(|err| anyhow::anyhow!("Failed to insert into trader table: {err}"))?;

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
            .map_err(|err| anyhow::anyhow!("Failed to insert into client table: {err}"))?;
        }

        sqlx::query(r#"
            INSERT INTO "order_event" (
                id, kind, order_id, order_type, order_side, trader_id, client_id, strategy_id, instrument_id, trade_id, currency, quantity, time_in_force, liquidity_side,
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
                kind = $2, order_id = $3, order_type = $4, order_side=$5, trader_id = $6, client_id = $7, strategy_id = $8, instrument_id = $9, trade_id = $10, currency = $11,
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
            .map_err(|err| anyhow::anyhow!("Failed to insert into order_event table: {err}"))?;
        transaction
            .commit()
            .await
            .map_err(|err| anyhow::anyhow!("Failed to commit transaction: {err}"))
    }

    pub async fn load_order_events(
        pool: &PgPool,
        order_id: &ClientOrderId,
    ) -> anyhow::Result<Vec<OrderEventAny>> {
        sqlx::query_as::<_, OrderEventAnyModel>(r#"SELECT * FROM "order_event" event WHERE event.order_id = $1 ORDER BY created_at ASC"#)
        .bind(order_id.to_string())
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|row| row.0).collect())
        .map_err(|err| anyhow::anyhow!("Failed to load order events: {err}"))
    }

    pub async fn load_order(
        pool: &PgPool,
        order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        let order_events = DatabaseQueries::load_order_events(pool, order_id).await;

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
            SELECT DISTINCT order_id FROM "order_event"
        "#,
        )
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| ClientOrderId::from(row.get::<&str, _>(0)))
                .collect()
        })
        .map_err(|err| anyhow::anyhow!("Failed to load order ids: {err}"))?;
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
        .map_err(|err| anyhow::anyhow!("Failed to insert into account table: {err}"))?;

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
            .map_err(|err| anyhow::anyhow!("Failed to insert into account_event table: {err}"))?;
        transaction
            .commit()
            .await
            .map_err(|err| anyhow::anyhow!("Failed to commit add_account transaction: {err}"))
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
        .map_err(|err| anyhow::anyhow!("Failed to load account events: {err}"))
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
        .map_err(|err| anyhow::anyhow!("Failed to load account ids: {err}"))?;
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
            .map_err(|err| anyhow::anyhow!("Failed to insert into trade table: {err}"))
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
        .map_err(|err| anyhow::anyhow!("Failed to load trades: {err}"))
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
            .map_err(|err| anyhow::anyhow!("Failed to insert into quote table: {err}"))
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
        .map_err(|err| anyhow::anyhow!("Failed to load quotes: {err}"))
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
            .bind(bar.bar_type.spec().step as i32)
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
            .map_err(|err| anyhow::anyhow!("Failed to insert into bar table: {err}"))
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
        .map_err(|err| anyhow::anyhow!("Failed to load bars: {err}"))
    }

    pub async fn load_distinct_order_event_client_ids(
        pool: &PgPool,
    ) -> anyhow::Result<HashMap<ClientOrderId, ClientId>> {
        let mut map: HashMap<ClientOrderId, ClientId> = HashMap::new();
        let result = sqlx::query_as::<_, OrderEventOrderClientIdCombination>(
            r#"
            SELECT DISTINCT
                order_id AS "order_id",
                client_id AS "client_id"
            FROM "order_event"
        "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|err| anyhow::anyhow!("Failed to load account ids: {err}"))?;
        for id in result {
            map.insert(id.order_id, id.client_id);
        }
        Ok(map)
    }
}
