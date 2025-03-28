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

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use arrow::array::RecordBatch;
use chrono::{DateTime, Duration, NaiveDate};
use futures_util::{StreamExt, future::join_all, pin_mut};
use heck::ToSnakeCase;
use nautilus_core::{UnixNanos, parsing::precision_from_str};
use nautilus_model::{
    data::{
        Bar, BarType, Data, OrderBookDelta, OrderBookDeltas_API, OrderBookDepth10, QuoteTick,
        TradeTick,
    },
    identifiers::InstrumentId,
};
use nautilus_serialization::{
    arrow::{
        bars_to_arrow_record_batch_bytes, order_book_deltas_to_arrow_record_batch_bytes,
        order_book_depth10_to_arrow_record_batch_bytes, quote_ticks_to_arrow_record_batch_bytes,
        trade_ticks_to_arrow_record_batch_bytes,
    },
    parquet::write_batch_to_parquet,
};
use thousands::Separable;
use ustr::Ustr;

use super::{enums::Exchange, http::models::InstrumentInfo};
use crate::{
    config::TardisReplayConfig,
    http::TardisHttpClient,
    machine::{TardisMachineClient, types::InstrumentMiniInfo},
    parse::{normalize_instrument_id, parse_instrument_id},
};

struct DateCursor {
    /// Cursor date UTC.
    date_utc: NaiveDate,
    /// Cursor end timestamp UNIX nanoseconds.
    end_ns: UnixNanos,
}

impl DateCursor {
    /// Creates a new [`DateCursor`] instance.
    fn new(current_ns: UnixNanos) -> Self {
        let current_utc = DateTime::from_timestamp_nanos(current_ns.as_i64());
        let date_utc = current_utc.date_naive();

        // Calculate end of the current UTC day
        // SAFETY: Known safe input values
        let end_utc =
            date_utc.and_hms_opt(23, 59, 59).unwrap() + Duration::nanoseconds(999_999_999);
        let end_ns = UnixNanos::from(end_utc.and_utc().timestamp_nanos_opt().unwrap() as u64);

        Self { date_utc, end_ns }
    }
}

async fn gather_instruments_info(
    config: &TardisReplayConfig,
    http_client: &TardisHttpClient,
) -> HashMap<Exchange, Vec<InstrumentInfo>> {
    let futures = config.options.iter().map(|options| {
        let exchange = options.exchange.clone();
        let client = &http_client;

        tracing::info!("Requesting instruments for {exchange}");

        async move {
            match client.instruments_info(exchange.clone(), None, None).await {
                Ok(instruments) => Some((exchange, instruments)),
                Err(e) => {
                    tracing::error!("Error fetching instruments for {exchange}: {e}");
                    None
                }
            }
        }
    });

    let results: Vec<(Exchange, Vec<InstrumentInfo>)> =
        join_all(futures).await.into_iter().flatten().collect();

    tracing::info!("Received all instruments");

    results.into_iter().collect()
}

pub async fn run_tardis_machine_replay_from_config(config_filepath: &Path) -> anyhow::Result<()> {
    tracing::info!("Starting replay");
    tracing::info!("Config filepath: {config_filepath:?}");

    let config_data = fs::read_to_string(config_filepath).expect("Failed to read config file");
    let config: TardisReplayConfig =
        serde_json::from_str(&config_data).expect("Failed to parse config JSON");

    let path = config
        .output_path
        .as_deref()
        .map(Path::new)
        .map(Path::to_path_buf)
        .or_else(|| {
            std::env::var("NAUTILUS_CATALOG_PATH")
                .ok()
                .map(|env_path| PathBuf::from(env_path).join("data"))
        })
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    tracing::info!("Output path: {path:?}");

    let normalize_symbols = config.normalize_symbols.unwrap_or(true);
    tracing::info!("normalize_symbols={normalize_symbols}");

    let http_client = TardisHttpClient::new(None, None, None, normalize_symbols)?;
    let mut machine_client =
        TardisMachineClient::new(config.tardis_ws_url.as_deref(), normalize_symbols)?;

    let info_map = gather_instruments_info(&config, &http_client).await;

    for (exchange, instruments) in &info_map {
        for inst in instruments {
            let instrument_type = inst.instrument_type.clone();
            let price_precision = precision_from_str(&inst.price_increment.to_string());
            let size_precision = precision_from_str(&inst.amount_increment.to_string());

            let instrument_id = if normalize_symbols {
                normalize_instrument_id(exchange, inst.id, &instrument_type, inst.inverse)
            } else {
                parse_instrument_id(exchange, inst.id)
            };

            let info = InstrumentMiniInfo::new(
                instrument_id,
                Some(Ustr::from(&inst.id)),
                exchange.clone(),
                price_precision,
                size_precision,
            );
            machine_client.add_instrument_info(info);
        }
    }

    tracing::info!("Starting tardis-machine stream");
    let stream = machine_client.replay(config.options).await;
    pin_mut!(stream);

    // Initialize date cursors
    let mut deltas_cursors: HashMap<InstrumentId, DateCursor> = HashMap::new();
    let mut depths_cursors: HashMap<InstrumentId, DateCursor> = HashMap::new();
    let mut quotes_cursors: HashMap<InstrumentId, DateCursor> = HashMap::new();
    let mut trades_cursors: HashMap<InstrumentId, DateCursor> = HashMap::new();
    let mut bars_cursors: HashMap<BarType, DateCursor> = HashMap::new();

    // Initialize date collection maps
    let mut deltas_map: HashMap<InstrumentId, Vec<OrderBookDelta>> = HashMap::new();
    let mut depths_map: HashMap<InstrumentId, Vec<OrderBookDepth10>> = HashMap::new();
    let mut quotes_map: HashMap<InstrumentId, Vec<QuoteTick>> = HashMap::new();
    let mut trades_map: HashMap<InstrumentId, Vec<TradeTick>> = HashMap::new();
    let mut bars_map: HashMap<BarType, Vec<Bar>> = HashMap::new();

    let mut msg_count = 0;

    while let Some(msg) = stream.next().await {
        match msg {
            Data::Deltas(msg) => {
                handle_deltas_msg(msg, &mut deltas_map, &mut deltas_cursors, &path);
            }
            Data::Depth10(msg) => {
                handle_depth10_msg(*msg, &mut depths_map, &mut depths_cursors, &path);
            }
            Data::Quote(msg) => handle_quote_msg(msg, &mut quotes_map, &mut quotes_cursors, &path),
            Data::Trade(msg) => handle_trade_msg(msg, &mut trades_map, &mut trades_cursors, &path),
            Data::Bar(msg) => handle_bar_msg(msg, &mut bars_map, &mut bars_cursors, &path),
            Data::Delta(_) => panic!("Individual delta message not implemented (or required)"),
        }

        msg_count += 1;
        if msg_count % 100_000 == 0 {
            tracing::debug!("Processed {} messages", msg_count.separate_with_commas());
        }
    }

    // Naively iterate through every remaining type and instrument sequentially

    for (instrument_id, deltas) in deltas_map {
        let cursor = deltas_cursors.get(&instrument_id).expect("Expected cursor");
        batch_and_write_deltas(deltas, &instrument_id, cursor.date_utc, &path);
    }

    for (instrument_id, depths) in depths_map {
        let cursor = depths_cursors.get(&instrument_id).expect("Expected cursor");
        batch_and_write_depths(depths, &instrument_id, cursor.date_utc, &path);
    }

    for (instrument_id, quotes) in quotes_map {
        let cursor = quotes_cursors.get(&instrument_id).expect("Expected cursor");
        batch_and_write_quotes(quotes, &instrument_id, cursor.date_utc, &path);
    }

    for (instrument_id, trades) in trades_map {
        let cursor = trades_cursors.get(&instrument_id).expect("Expected cursor");
        batch_and_write_trades(trades, &instrument_id, cursor.date_utc, &path);
    }

    for (bar_type, bars) in bars_map {
        let cursor = bars_cursors.get(&bar_type).expect("Expected cursor");
        batch_and_write_bars(bars, &bar_type, cursor.date_utc, &path);
    }

    tracing::info!(
        "Replay completed after {} messages",
        msg_count.separate_with_commas()
    );
    Ok(())
}

fn handle_deltas_msg(
    deltas: OrderBookDeltas_API,
    map: &mut HashMap<InstrumentId, Vec<OrderBookDelta>>,
    cursors: &mut HashMap<InstrumentId, DateCursor>,
    path: &Path,
) {
    let cursor = cursors
        .entry(deltas.instrument_id)
        .or_insert_with(|| DateCursor::new(deltas.ts_init));

    if deltas.ts_init > cursor.end_ns {
        if let Some(deltas_vec) = map.remove(&deltas.instrument_id) {
            batch_and_write_deltas(deltas_vec, &deltas.instrument_id, cursor.date_utc, path);
        }
        // Update cursor
        *cursor = DateCursor::new(deltas.ts_init);
    }

    map.entry(deltas.instrument_id)
        .or_insert_with(|| Vec::with_capacity(1_000_000))
        .extend(&*deltas.deltas);
}

fn handle_depth10_msg(
    depth10: OrderBookDepth10,
    map: &mut HashMap<InstrumentId, Vec<OrderBookDepth10>>,
    cursors: &mut HashMap<InstrumentId, DateCursor>,
    path: &Path,
) {
    let cursor = cursors
        .entry(depth10.instrument_id)
        .or_insert_with(|| DateCursor::new(depth10.ts_init));

    if depth10.ts_init > cursor.end_ns {
        if let Some(depths_vec) = map.remove(&depth10.instrument_id) {
            batch_and_write_depths(depths_vec, &depth10.instrument_id, cursor.date_utc, path);
        }
        // Update cursor
        *cursor = DateCursor::new(depth10.ts_init);
    }

    map.entry(depth10.instrument_id)
        .or_insert_with(|| Vec::with_capacity(1_000_000))
        .push(depth10);
}

fn handle_quote_msg(
    quote: QuoteTick,
    map: &mut HashMap<InstrumentId, Vec<QuoteTick>>,
    cursors: &mut HashMap<InstrumentId, DateCursor>,
    path: &Path,
) {
    let cursor = cursors
        .entry(quote.instrument_id)
        .or_insert_with(|| DateCursor::new(quote.ts_init));

    if quote.ts_init > cursor.end_ns {
        if let Some(quotes_vec) = map.remove(&quote.instrument_id) {
            batch_and_write_quotes(quotes_vec, &quote.instrument_id, cursor.date_utc, path);
        }
        // Update cursor
        *cursor = DateCursor::new(quote.ts_init);
    }

    map.entry(quote.instrument_id)
        .or_insert_with(|| Vec::with_capacity(1_000_000))
        .push(quote);
}

fn handle_trade_msg(
    trade: TradeTick,
    map: &mut HashMap<InstrumentId, Vec<TradeTick>>,
    cursors: &mut HashMap<InstrumentId, DateCursor>,
    path: &Path,
) {
    let cursor = cursors
        .entry(trade.instrument_id)
        .or_insert_with(|| DateCursor::new(trade.ts_init));

    if trade.ts_init > cursor.end_ns {
        if let Some(trades_vec) = map.remove(&trade.instrument_id) {
            batch_and_write_trades(trades_vec, &trade.instrument_id, cursor.date_utc, path);
        }
        // Update cursor
        *cursor = DateCursor::new(trade.ts_init);
    }

    map.entry(trade.instrument_id)
        .or_insert_with(|| Vec::with_capacity(1_000_000))
        .push(trade);
}

fn handle_bar_msg(
    bar: Bar,
    map: &mut HashMap<BarType, Vec<Bar>>,
    cursors: &mut HashMap<BarType, DateCursor>,
    path: &Path,
) {
    let cursor = cursors
        .entry(bar.bar_type)
        .or_insert_with(|| DateCursor::new(bar.ts_init));

    if bar.ts_init > cursor.end_ns {
        if let Some(bars_vec) = map.remove(&bar.bar_type) {
            batch_and_write_bars(bars_vec, &bar.bar_type, cursor.date_utc, path);
        }
        // Update cursor
        *cursor = DateCursor::new(bar.ts_init);
    }

    map.entry(bar.bar_type)
        .or_insert_with(|| Vec::with_capacity(1_000_000))
        .push(bar);
}

fn batch_and_write_deltas(
    deltas: Vec<OrderBookDelta>,
    instrument_id: &InstrumentId,
    date: NaiveDate,
    path: &Path,
) {
    let typename = stringify!(OrderBookDeltas);
    match order_book_deltas_to_arrow_record_batch_bytes(deltas) {
        Ok(batch) => write_batch(batch, typename, instrument_id, date, path),
        Err(e) => {
            tracing::error!("Error converting `{typename}` to Arrow: {e:?}");
        }
    }
}

fn batch_and_write_depths(
    depths: Vec<OrderBookDepth10>,
    instrument_id: &InstrumentId,
    date: NaiveDate,
    path: &Path,
) {
    let typename = stringify!(OrderBookDepth10);
    match order_book_depth10_to_arrow_record_batch_bytes(depths) {
        Ok(batch) => write_batch(batch, typename, instrument_id, date, path),
        Err(e) => {
            tracing::error!("Error converting `{typename}` to Arrow: {e:?}");
        }
    }
}

fn batch_and_write_quotes(
    quotes: Vec<QuoteTick>,
    instrument_id: &InstrumentId,
    date: NaiveDate,
    path: &Path,
) {
    let typename = stringify!(QuoteTick);
    match quote_ticks_to_arrow_record_batch_bytes(quotes) {
        Ok(batch) => write_batch(batch, typename, instrument_id, date, path),
        Err(e) => {
            tracing::error!("Error converting `{typename}` to Arrow: {e:?}");
        }
    }
}

fn batch_and_write_trades(
    trades: Vec<TradeTick>,
    instrument_id: &InstrumentId,
    date: NaiveDate,
    path: &Path,
) {
    let typename = stringify!(TradeTick);
    match trade_ticks_to_arrow_record_batch_bytes(trades) {
        Ok(batch) => write_batch(batch, typename, instrument_id, date, path),
        Err(e) => {
            tracing::error!("Error converting `{typename}` to Arrow: {e:?}");
        }
    }
}

fn batch_and_write_bars(bars: Vec<Bar>, bar_type: &BarType, date: NaiveDate, path: &Path) {
    let typename = stringify!(Bar);
    let batch = match bars_to_arrow_record_batch_bytes(bars) {
        Ok(batch) => batch,
        Err(e) => {
            tracing::error!("Error converting `{typename}` to Arrow: {e:?}");
            return;
        }
    };

    let filepath = path.join(parquet_filepath_bars(bar_type, date));
    match write_batch_to_parquet(batch, &filepath, None, None, None) {
        Ok(()) => tracing::info!("File written: {filepath:?}"),
        Err(e) => tracing::error!("Error writing {filepath:?}: {e:?}"),
    }
}

fn parquet_filepath(typename: &str, instrument_id: &InstrumentId, date: NaiveDate) -> PathBuf {
    let typename = typename.to_snake_case();
    let instrument_id_str = instrument_id.to_string().replace('/', "");
    let date_str = date.to_string().replace('-', "");
    PathBuf::new()
        .join(typename)
        .join(instrument_id_str)
        .join(format!("{date_str}.parquet"))
}

fn parquet_filepath_bars(bar_type: &BarType, date: NaiveDate) -> PathBuf {
    let bar_type_str = bar_type.to_string().replace('/', "");
    let date_str = date.to_string().replace('-', "");
    PathBuf::new()
        .join("bar")
        .join(bar_type_str)
        .join(format!("{date_str}.parquet"))
}

fn write_batch(
    batch: RecordBatch,
    typename: &str,
    instrument_id: &InstrumentId,
    date: NaiveDate,
    path: &Path,
) {
    let filepath = path.join(parquet_filepath(typename, instrument_id, date));
    match write_batch_to_parquet(batch, &filepath, None, None, None) {
        Ok(()) => tracing::info!("File written: {filepath:?}"),
        Err(e) => tracing::error!("Error writing {filepath:?}: {e:?}"),
    }
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// Tests
///////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
    // Start of day: 2024-01-01 00:00:00 UTC
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap().timestamp_nanos_opt().unwrap() as u64,
    NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
    Utc.with_ymd_and_hms(2024, 1, 1, 23, 59, 59).unwrap().timestamp_nanos_opt().unwrap() as u64 + 999_999_999
)]
    #[case(
    // Midday: 2024-01-01 12:00:00 UTC
    Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().timestamp_nanos_opt().unwrap() as u64,
    NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
    Utc.with_ymd_and_hms(2024, 1, 1, 23, 59, 59).unwrap().timestamp_nanos_opt().unwrap() as u64 + 999_999_999
)]
    #[case(
    // End of day: 2024-01-01 23:59:59.999999999 UTC
    Utc.with_ymd_and_hms(2024, 1, 1, 23, 59, 59).unwrap().timestamp_nanos_opt().unwrap() as u64 + 999_999_999,
    NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
    Utc.with_ymd_and_hms(2024, 1, 1, 23, 59, 59).unwrap().timestamp_nanos_opt().unwrap() as u64 + 999_999_999
)]
    #[case(
    // Start of new day: 2024-01-02 00:00:00 UTC
    Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap().timestamp_nanos_opt().unwrap() as u64,
    NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
    Utc.with_ymd_and_hms(2024, 1, 2, 23, 59, 59).unwrap().timestamp_nanos_opt().unwrap() as u64 + 999_999_999
)]
    fn test_date_cursor(
        #[case] timestamp: u64,
        #[case] expected_date: NaiveDate,
        #[case] expected_end_ns: u64,
    ) {
        let unix_nanos = UnixNanos::from(timestamp);
        let cursor = DateCursor::new(unix_nanos);

        assert_eq!(cursor.date_utc, expected_date);
        assert_eq!(cursor.end_ns, UnixNanos::from(expected_end_ns));
    }
}
