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

//! Collects scoped REST execution snapshots and recovery evidence.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
};

use nautilus_core::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_SECOND, try_datetime_to_unix_nanos},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{identifiers::InstrumentId, instruments::BinaryOption, orders::OrderAny};

use crate::{
    common::enums::{OrderStatusFilter, PositionSettlementStatus},
    http::{
        client::KalshiHttpClient,
        error::KalshiHttpError,
        models::{Field, Fill, HistoricalCutoff, MarketPosition, Order, OrderStatus},
        parse::Pagination,
        query::{
            AccountQuery, FillsQuery, HistoricalPositionsQuery, HistoryQuery, OrdersQuery,
            PositionsQuery,
        },
    },
};

const RECOVERY_MAX_PAGES: u32 = 100;

/// Complete REST source collection for one subaccount and shard.
///
/// Records retain the venue's values and omissions. Separate requests are not an atomic account
/// snapshot. Settled positions are historical evidence, not current exposure or synthetic fills.
/// For nonzero subaccounts, orders and fills require explicit matching subaccount identity.
#[derive(Debug)]
pub struct KalshiExecutionSnapshot {
    /// Orders retained for the shard, including resting orders, orders inside the lookback,
    /// and orders linked to retained fills.
    pub orders: Vec<Order>,
    /// Fills for the shard within the optional lookback and no later than `ts_init`.
    pub fills: Vec<Fill>,
    /// Current positions for the shard from the live positions endpoint.
    pub positions: Vec<MarketPosition>,
    /// Settled-position evidence for the shard from live and historical sources, not current exposure.
    pub positions_settled: Vec<MarketPosition>,
    /// Venue cutoffs used to route live and historical collection.
    pub cutoff: HistoricalCutoff,
    /// Inclusive start of the order and fill lookback, or `None` for unbounded history.
    pub lookback_start: Option<UnixNanos>,
    /// Timestamp captured before any source requests and preserved unchanged.
    pub ts_init: UnixNanos,
}

/// Collects execution evidence without inventing missing command metadata.
///
/// Open orders and current positions have no history bound. Closed orders include every collected
/// fill's order, including orders updated before the requested window. Live reads precede archive
/// reads to cover forward handoffs. Each source must exhaust pagination; conflicts fail collection.
/// The original initialization timestamp precedes all requests and survives collection unchanged.
///
/// # Errors
///
/// Returns an error if bounds or timestamps are invalid, pagination is incomplete, records conflict
/// or lack scope, fills lack order evidence, or any source fails.
pub async fn collect_snapshot(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    lookback_start: Option<UnixNanos>,
    max_pages: u32,
) -> Result<KalshiExecutionSnapshot, KalshiHttpError> {
    let ts_init = get_atomic_clock_realtime().get_time_ns();
    collect_snapshot_at(http, account, lookback_start, max_pages, ts_init).await
}

pub(super) async fn collect_snapshot_at(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    lookback_start: Option<UnixNanos>,
    max_pages: u32,
    ts_init: UnixNanos,
) -> Result<KalshiExecutionSnapshot, KalshiHttpError> {
    if max_pages == 0 || lookback_start.is_some_and(|start| start > ts_init) {
        return Err(KalshiHttpError::Validation(
            "invalid execution snapshot window or page budget",
        ));
    }

    let cutoff = http.get_historical_cutoff().await?;

    if !matches!(cutoff.market_positions_last_updated_ts, Field::Value(_)) {
        return Err(KalshiHttpError::Parse(
            "missing position history coverage".into(),
        ));
    }

    let orders = collect_orders(http, account, None, max_pages).await?;

    let retained_fills = collect_fills(
        http,
        &FillsQuery {
            exchange_index: account.exchange_index,
            subaccount: account.subaccount,
            ..Default::default()
        },
        lookback_start,
        ts_init,
        &cutoff,
        max_pages,
    )
    .await?;

    let retained_orders = retain_orders(orders, &retained_fills, lookback_start, &[])?;

    let (positions, positions_settled) = collect_positions(http, account, None, max_pages).await?;
    Ok(KalshiExecutionSnapshot {
        orders: retained_orders,
        fills: retained_fills,
        positions,
        positions_settled,
        cutoff,
        lookback_start,
        ts_init,
    })
}

fn retain_orders(
    orders: HashMap<String, Order>,
    fills: &[Fill],
    lookback_start: Option<UnixNanos>,
    owned: &[OrderAny],
) -> Result<Vec<Order>, KalshiHttpError> {
    use nautilus_model::orders::Order as _;

    let venue_ids: Vec<_> = owned
        .iter()
        .filter_map(|order| order.venue_order_id())
        .collect();
    let mut required_orders: HashSet<_> = venue_ids.iter().map(|id| id.as_str()).collect();

    for fill in fills {
        let Some(order) = orders.get(&fill.order_id) else {
            return Err(KalshiHttpError::Parse(
                "fill has no authoritative order evidence".into(),
            ));
        };

        if order.ticker != fill.ticker || fill.ticker != fill.market_ticker {
            return Err(KalshiHttpError::Parse(
                "fill and order market identities differ".into(),
            ));
        }

        required_orders.insert(fill.order_id.as_str());
    }

    let mut retained_orders = Vec::new();

    for order in orders.into_values() {
        let retain = order.status == OrderStatus::Resting
            || required_orders.contains(order.order_id.as_str());
        if retain || lookback_start.is_none() {
            retained_orders.push(order);
            continue;
        }

        let timestamp = timestamp(&order.last_update_time)?;
        if lookback_start.is_some_and(|start| timestamp >= start) {
            retained_orders.push(order);
        }
    }

    retained_orders.sort_by(|a, b| a.order_id.cmp(&b.order_id));
    Ok(retained_orders)
}

pub(super) async fn collect_recovery_snapshot(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    start: UnixNanos,
    tracked: &[InstrumentId],
    owned: &[OrderAny],
) -> Result<KalshiExecutionSnapshot, KalshiHttpError> {
    use nautilus_model::orders::Order as _;

    let max_pages = RECOVERY_MAX_PAGES;
    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let cutoff = http.get_historical_cutoff().await?;

    if !matches!(cutoff.market_positions_last_updated_ts, Field::Value(_)) {
        return Err(KalshiHttpError::Parse(
            "missing position history coverage".into(),
        ));
    }

    let mut orders = collect_orders_window(http, account, None, Some(start), max_pages).await?;

    let resting = collect_orders_live(
        http,
        &OrdersQuery {
            status: Some(OrderStatusFilter::Resting),
            exchange_index: account.exchange_index,
            subaccount: account.subaccount,
            ..Default::default()
        },
        max_pages,
    )
    .await?;

    for order in resting.into_values() {
        merge(&mut orders, order.order_id.clone(), order)?;
    }

    let mut fills = collect_fills(
        http,
        &FillsQuery {
            exchange_index: account.exchange_index,
            subaccount: account.subaccount,
            ..Default::default()
        },
        Some(start),
        ts_init,
        &cutoff,
        max_pages,
    )
    .await?;

    for context in owned {
        if let Some(id) = context.venue_order_id() {
            recover_order(
                http,
                account,
                id.as_str(),
                context.instrument_id().symbol.as_str(),
                max_pages,
                &mut orders,
            )
            .await?;
        }
    }

    for fill in &fills {
        recover_order(
            http,
            account,
            &fill.order_id,
            &fill.ticker,
            max_pages,
            &mut orders,
        )
        .await?;
    }

    recover_order_fills(http, account, &orders, owned, &cutoff, ts_init, &mut fills).await?;
    let orders = retain_orders(orders, &fills, Some(start), owned)?;
    let positions = collect_positions_current(http, account, None, max_pages).await?;
    let mut targets: HashSet<_> = tracked.iter().map(|id| id.symbol.to_string()).collect();
    targets.extend(fills.iter().map(|fill| fill.ticker.clone()));
    for position in &positions {
        targets.remove(&position.ticker);
    }

    let mut positions_settled = Vec::new();
    for ticker in targets {
        positions_settled
            .extend(collect_positions_settled(http, account, Some(&ticker), max_pages).await?);
    }

    positions_settled.sort_by(|a, b| a.ticker.cmp(&b.ticker));
    Ok(KalshiExecutionSnapshot {
        orders,
        fills,
        positions,
        positions_settled,
        cutoff,
        lookback_start: Some(start),
        ts_init,
    })
}

async fn recover_order(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    id: &str,
    ticker: &str,
    max_pages: u32,
    orders: &mut HashMap<String, Order>,
) -> Result<(), KalshiHttpError> {
    if orders.contains_key(id) {
        return Ok(());
    }

    match http.get_order(id).await {
        Ok(response) => {
            validate_order_scope(&response.order, account, false)?;
            merge(orders, response.order.order_id.clone(), response.order)?;
        }
        Err(
            KalshiHttpError::Status(404)
            | KalshiHttpError::Http { status: 404, .. }
            | KalshiHttpError::Venue { status: 404, .. },
        ) => {
            for order in collect_orders(http, account, Some(ticker), max_pages)
                .await?
                .into_values()
            {
                merge(orders, order.order_id.clone(), order)?;
            }
        }
        Err(e) => return Err(e),
    }

    Ok(())
}

async fn recover_order_fills(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    orders: &HashMap<String, Order>,
    owned: &[OrderAny],
    cutoff: &HistoricalCutoff,
    ts_init: UnixNanos,
    fills: &mut Vec<Fill>,
) -> Result<(), KalshiHttpError> {
    use nautilus_model::orders::Order as _;
    let mut retained: HashMap<_, _> = fills
        .drain(..)
        .map(|fill| (fill.fill_id.clone(), fill))
        .collect();

    for context in owned {
        let Some(id) = context.venue_order_id() else {
            continue;
        };

        let Some(order) = orders.get(id.as_str()) else {
            continue;
        };

        if order.fill_count_fp.0 <= context.filled_qty().as_decimal() {
            continue;
        }

        let query = FillsQuery {
            ticker: Some(order.ticker.clone()),
            order_id: Some(order.order_id.clone()),
            exchange_index: account.exchange_index,
            subaccount: account.subaccount,
            ..Default::default()
        };

        for fill in collect_fills(
            http,
            &query,
            Some(context.ts_init()),
            ts_init,
            cutoff,
            RECOVERY_MAX_PAGES,
        )
        .await?
        {
            if fill.order_id == order.order_id {
                merge(&mut retained, fill.fill_id.clone(), fill)?;
            }
        }
    }

    *fills = retained.into_values().collect();
    fills.sort_by(|a, b| a.fill_id.cmp(&b.fill_id));
    Ok(())
}

pub(super) async fn collect_orders(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    ticker: Option<&str>,
    max_pages: u32,
) -> Result<HashMap<String, Order>, KalshiHttpError> {
    collect_orders_window(http, account, ticker, None, max_pages).await
}

async fn collect_orders_window(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    ticker: Option<&str>,
    start: Option<UnixNanos>,
    max_pages: u32,
) -> Result<HashMap<String, Order>, KalshiHttpError> {
    let min_ts =
        start.map(|start| (start.as_u64() / NANOSECONDS_IN_SECOND).saturating_sub(1) as i64);

    let mut orders = collect_orders_live(
        http,
        &OrdersQuery {
            ticker: ticker.map(String::from),
            min_ts,
            exchange_index: account.exchange_index,
            subaccount: account.subaccount,
            ..Default::default()
        },
        max_pages,
    )
    .await?;

    let mut archived_orders = HashMap::new();
    collect_pages(
        max_pages,
        &mut archived_orders,
        |order: &Order| order.order_id.clone(),
        |cursor| async move {
            let page = http
                .get_historical_orders(&HistoryQuery {
                    ticker: ticker.map(String::from),
                    min_ts,
                    cursor,
                    limit: Some(1000),
                    subaccount: account.subaccount,
                    ..Default::default()
                })
                .await?;

            let empty = page.orders.is_empty();
            Ok((page.orders, page.cursor, empty))
        },
    )
    .await?;

    for order in archived_orders.into_values() {
        if validate_order_scope(&order, account, true)? {
            merge(&mut orders, order.order_id.clone(), order)?;
        }
    }

    Ok(orders)
}

async fn collect_orders_live(
    http: &KalshiHttpClient,
    query: &OrdersQuery,
    max_pages: u32,
) -> Result<HashMap<String, Order>, KalshiHttpError> {
    let mut orders = HashMap::new();
    collect_pages(
        max_pages,
        &mut orders,
        |order: &Order| order.order_id.clone(),
        |cursor| async move {
            let page = http
                .get_orders(&OrdersQuery {
                    cursor,
                    limit: Some(1000),
                    ..query.clone()
                })
                .await?;

            let empty = page.orders.is_empty();
            Ok((page.orders, page.cursor, empty))
        },
    )
    .await?;

    for order in orders.values() {
        validate_order_scope(
            order,
            &AccountQuery {
                subaccount: query.subaccount,
                exchange_index: query.exchange_index,
            },
            false,
        )?;
    }

    Ok(orders)
}

pub(super) async fn collect_fills(
    http: &KalshiHttpClient,
    query: &FillsQuery,
    lookback_start: Option<UnixNanos>,
    ts_init: UnixNanos,
    cutoff: &HistoricalCutoff,
    max_pages: u32,
) -> Result<Vec<Fill>, KalshiHttpError> {
    let account = AccountQuery {
        subaccount: query.subaccount,
        exchange_index: query.exchange_index,
    };

    let exchange_index = query.exchange_index;
    let start = lookback_start.map(|value| (value.as_u64() / NANOSECONDS_IN_SECOND) as i64);
    let end = (ts_init.as_u64() / NANOSECONDS_IN_SECOND + 1) as i64;
    let mut fills = HashMap::new();
    let live_start = Some(
        start
            .unwrap_or(0)
            .max(cutoff.trades_created_ts.as_second())
            .saturating_sub(1)
            .max(0),
    );
    collect_pages(
        max_pages,
        &mut fills,
        |fill: &Fill| format!("{}:{}", fill.exchange_index, fill.fill_id),
        |cursor| async move {
            let page = http
                .get_fills(&FillsQuery {
                    ticker: query.ticker.clone(),
                    order_id: query.order_id.clone(),
                    cursor,
                    limit: Some(1000),
                    min_ts: live_start,
                    max_ts: Some(end),
                    subaccount: account.subaccount,
                    exchange_index,
                })
                .await?;

            let empty = page.fills.is_empty();
            Ok((page.fills, page.cursor, empty))
        },
    )
    .await?;

    for fill in fills.values() {
        validate_fill_scope(fill, &account, false)?;
    }

    let mut archived_fills = HashMap::new();
    collect_pages(
        max_pages,
        &mut archived_fills,
        |fill: &Fill| format!("{}:{}", fill.exchange_index, fill.fill_id),
        |cursor| async move {
            let page = http
                .get_historical_fills(&HistoryQuery {
                    ticker: query.ticker.clone(),
                    cursor,
                    limit: Some(1000),
                    min_ts: start.map(|start| start.saturating_sub(1).max(0)),
                    max_ts: Some(end),
                    subaccount: account.subaccount,
                })
                .await?;

            let empty = page.fills.is_empty();
            Ok((page.fills, page.cursor, empty))
        },
    )
    .await?;

    for fill in archived_fills.into_values() {
        if validate_fill_scope(&fill, &account, true)? {
            let key = format!("{}:{}", fill.exchange_index, fill.fill_id);
            merge(&mut fills, key, fill)?;
        }
    }

    let mut retained_fills = Vec::new();

    for fill in fills.into_values() {
        let timestamp = timestamp(&fill.created_time)?;
        if lookback_start.is_none_or(|start| timestamp >= start) && timestamp <= ts_init {
            retained_fills.push((timestamp, fill));
        }
    }

    retained_fills.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.fill_id.cmp(&b.1.fill_id)));
    let retained_fills: Vec<_> = retained_fills.into_iter().map(|(_, fill)| fill).collect();
    Ok(retained_fills)
}

pub(super) async fn collect_positions_current(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    ticker: Option<&str>,
    max_pages: u32,
) -> Result<Vec<MarketPosition>, KalshiHttpError> {
    let mut positions = HashMap::new();
    collect_pages(
        max_pages,
        &mut positions,
        |position: &MarketPosition| position.ticker.clone(),
        |cursor| async move {
            let page = http
                .get_positions(&PositionsQuery {
                    ticker: ticker.map(String::from),
                    cursor,
                    limit: Some(1000),
                    subaccount: account.subaccount,
                    exchange_index: account.exchange_index,
                    ..Default::default()
                })
                .await?;

            let empty = page.market_positions.is_empty() && page.event_positions.is_empty();
            Ok((page.market_positions, page.cursor, empty))
        },
    )
    .await?;

    if positions
        .values()
        .any(|p| i64::from(p.exchange_index) != i64::from(account.exchange_index))
    {
        return Err(KalshiHttpError::Parse(
            "current position shard mismatch".into(),
        ));
    }

    let mut positions: Vec<_> = positions.into_values().collect();
    positions.sort_by(|a, b| a.ticker.cmp(&b.ticker));
    Ok(positions)
}

async fn collect_positions(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    ticker: Option<&str>,
    max_pages: u32,
) -> Result<(Vec<MarketPosition>, Vec<MarketPosition>), KalshiHttpError> {
    let positions = collect_positions_current(http, account, ticker, max_pages).await?;
    let positions_settled = collect_positions_settled(http, account, ticker, max_pages).await?;
    Ok((positions, positions_settled))
}

async fn collect_positions_settled(
    http: &KalshiHttpClient,
    account: &AccountQuery,
    ticker: Option<&str>,
    max_pages: u32,
) -> Result<Vec<MarketPosition>, KalshiHttpError> {
    let mut positions_settled = HashMap::new();
    collect_pages(
        max_pages,
        &mut positions_settled,
        |position: &MarketPosition| format!("{}:{}", position.exchange_index, position.ticker),
        |cursor| async move {
            let page = http
                .get_positions(&PositionsQuery {
                    ticker: ticker.map(String::from),
                    cursor,
                    limit: Some(1000),
                    settlement_status: Some(PositionSettlementStatus::Settled),
                    subaccount: account.subaccount,
                    exchange_index: account.exchange_index,
                    ..Default::default()
                })
                .await?;

            let empty = page.market_positions.is_empty() && page.event_positions.is_empty();
            Ok((page.market_positions, page.cursor, empty))
        },
    )
    .await?;

    if positions_settled
        .values()
        .any(|p| i64::from(p.exchange_index) != i64::from(account.exchange_index))
    {
        return Err(KalshiHttpError::Parse(
            "settled position shard mismatch".into(),
        ));
    }

    collect_pages(
        max_pages,
        &mut positions_settled,
        |position: &MarketPosition| format!("{}:{}", position.exchange_index, position.ticker),
        |cursor| async move {
            let page = http
                .get_historical_positions(&HistoricalPositionsQuery {
                    ticker: ticker.map(String::from),
                    cursor,
                    limit: Some(1000),
                    subaccount: account.subaccount,
                    ..Default::default()
                })
                .await?;

            let empty = page.market_positions.is_empty() && page.event_positions.is_empty();
            Ok((page.market_positions, page.cursor, empty))
        },
    )
    .await?;

    let mut positions_settled: Vec<_> = positions_settled
        .into_values()
        .filter(|p| i64::from(p.exchange_index) == i64::from(account.exchange_index))
        .collect();
    positions_settled.sort_by(|a, b| a.ticker.cmp(&b.ticker));
    Ok(positions_settled)
}

async fn collect_pages<T, F, Fut>(
    max_pages: u32,
    records: &mut HashMap<String, T>,
    identity: fn(&T) -> String,
    mut fetch: F,
) -> Result<(), KalshiHttpError>
where
    T: PartialEq,
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Field<String>, bool), KalshiHttpError>>,
{
    let mut cursor = None;
    let mut pagination = Pagination::new(None);

    for _ in 0..max_pages {
        let (page, next, empty) = fetch(cursor).await?;
        for record in page {
            let key = identity(&record);
            if key.is_empty() {
                return Err(KalshiHttpError::Parse(
                    "missing execution record identity".into(),
                ));
            }

            merge(records, key, record)?;
        }

        cursor = pagination.advance(next, empty)?;
        if cursor.is_none() {
            return Ok(());
        }
    }

    Err(KalshiHttpError::Pagination(
        "execution snapshot page budget exhausted",
    ))
}

fn merge<T: PartialEq>(
    records: &mut HashMap<String, T>,
    key: String,
    record: T,
) -> Result<(), KalshiHttpError> {
    if let Some(existing) = records.get(&key) {
        if *existing != record {
            return Err(KalshiHttpError::Parse(
                "conflicting execution record".into(),
            ));
        }
    } else {
        records.insert(key, record);
    }

    Ok(())
}

pub(super) fn validate_order_scope(
    order: &Order,
    account: &AccountQuery,
    archived: bool,
) -> Result<bool, KalshiHttpError> {
    if matches!(order.subaccount_number, Field::Value(value) if i64::from(value) != i64::from(account.subaccount))
    {
        return Err(KalshiHttpError::Parse("order subaccount mismatch".into()));
    }

    if account.subaccount != 0 && !matches!(order.subaccount_number, Field::Value(_)) {
        return Err(KalshiHttpError::Parse(
            "missing order subaccount identity".into(),
        ));
    }

    let Field::Value(shard) = order.exchange_index else {
        return Err(KalshiHttpError::Parse(
            "missing order shard identity".into(),
        ));
    };

    if i64::from(shard) == i64::from(account.exchange_index) {
        Ok(true)
    } else if archived {
        Ok(false)
    } else {
        Err(KalshiHttpError::Parse("live order shard mismatch".into()))
    }
}

fn validate_fill_scope(
    fill: &Fill,
    account: &AccountQuery,
    archived: bool,
) -> Result<bool, KalshiHttpError> {
    if fill.fill_id.is_empty() || fill.order_id.is_empty() || fill.trade_id.is_empty() {
        return Err(KalshiHttpError::Parse("missing fill identity".into()));
    }

    if matches!(fill.subaccount_number, Field::Value(value) if i64::from(value) != i64::from(account.subaccount))
    {
        return Err(KalshiHttpError::Parse("fill subaccount mismatch".into()));
    }

    if account.subaccount != 0 && !matches!(fill.subaccount_number, Field::Value(_)) {
        return Err(KalshiHttpError::Parse(
            "missing fill subaccount identity".into(),
        ));
    }

    if i64::from(fill.exchange_index) == i64::from(account.exchange_index) {
        Ok(true)
    } else if archived {
        Ok(false)
    } else {
        Err(KalshiHttpError::Parse("live fill shard mismatch".into()))
    }
}

pub(super) fn timestamp(value: &Field<jiff::Timestamp>) -> Result<UnixNanos, KalshiHttpError> {
    let Field::Value(value) = value else {
        return Err(KalshiHttpError::Parse("missing execution timestamp".into()));
    };

    let timestamp = try_datetime_to_unix_nanos(*value)
        .map_err(|_| KalshiHttpError::Parse("invalid execution timestamp".into()))?;

    if timestamp == UnixNanos::default() {
        return Err(KalshiHttpError::Parse("invalid execution timestamp".into()));
    }

    Ok(timestamp)
}

pub(super) async fn load_definition(
    http: &KalshiHttpClient,
    ticker: &str,
    ts_init: UnixNanos,
) -> anyhow::Result<BinaryOption> {
    let market = crate::providers::KalshiInstrumentProvider::load_market(http, ticker).await?;
    anyhow::ensure!(
        market.ticker == ticker,
        "requested market identity mismatch"
    );

    let event = http
        .get_event(
            &market.event_ticker,
            &crate::http::query::EventQuery {
                exclude_markets: Some(true),
                ..Default::default()
            },
        )
        .await?
        .event;

    let series = http.get_series(&event.series_ticker).await?.series;
    crate::providers::KalshiInstrumentProvider::parse(&market, &event, &series, ts_init)
}

pub(super) fn validate_definition(
    instrument: &BinaryOption,
    exchange_index: u32,
) -> anyhow::Result<()> {
    let shard = instrument
        .info
        .as_ref()
        .and_then(|info| info.get("market"))
        .and_then(|market| market.get("exchange_index"))
        .and_then(serde_json::Value::as_u64);
    anyhow::ensure!(
        shard == Some(u64::from(exchange_index)),
        "report instrument shard mismatch"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Field::Value(7), None)]
    #[case(Field::Value(0), Some("subaccount mismatch"))]
    #[case(Field::Value(-1), Some("subaccount mismatch"))]
    #[case(Field::Missing, Some("missing"))]
    #[case(Field::Null, Some("missing"))]
    fn numbered_records_require_matching_account(
        #[case] identity: Field<i32>,
        #[case] error: Option<&str>,
    ) {
        let f: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut order: Order = serde_json::from_value(f["order"]["order"].clone()).unwrap();
        let mut fill: Fill = serde_json::from_value(f["fills"]["fills"][0].clone()).unwrap();
        order.subaccount_number = identity.clone();
        fill.subaccount_number = identity;

        let account = AccountQuery {
            subaccount: 7,
            exchange_index: 3,
        };

        for (kind, result) in [
            ("order", validate_order_scope(&order, &account, false)),
            ("fill", validate_fill_scope(&fill, &account, false)),
        ] {
            match error {
                None => assert!(result.unwrap()),
                Some("missing") => assert_eq!(
                    result.unwrap_err().to_string(),
                    format!("parse error: missing {kind} subaccount identity")
                ),
                Some(_) => assert_eq!(
                    result.unwrap_err().to_string(),
                    format!("parse error: {kind} subaccount mismatch")
                ),
            }
        }
    }
}
