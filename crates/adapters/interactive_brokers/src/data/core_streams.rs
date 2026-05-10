// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use std::{fmt::Debug, sync::Arc, time::Duration};

use ahash::AHashMap;
use anyhow::Context;
use ibapi::{
    contracts::tick_types::TickType,
    market_data::{
        TradingHours,
        historical::{
            Bar as HistoricalBar, BarSize as HistoricalBarSize, HistoricalBarUpdate,
            WhatToShow as HistoricalWhatToShow,
        },
        realtime::{
            Bar as RealtimeBar, BarSize as RealtimeBarSize, MarketDepths, TickGeneric, TickPrice,
            TickPriceSize, TickSize, TickTypes, WhatToShow as RealtimeWhatToShow,
        },
    },
    subscriptions::Subscription,
};
use nautilus_common::messages::DataEvent;
use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, Data, OrderBookDelta, QuoteTick, option_chain::OptionGreeks},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use tokio_util::sync::CancellationToken;

use crate::data::{
    cache::{OptionGreeksCache, QuoteCache},
    convert::{ib_bar_to_nautilus_bar, ib_timestamp_to_unix_nanos},
    parse::{
        parse_index_price, parse_market_depth_operation, parse_option_open_interest,
        parse_quote_tick, parse_trade_tick,
    },
};

enum StreamAction {
    Continue,
    Stop,
}

const HISTORICAL_BAR_MIN_COUNT: i64 = 300;
const HISTORICAL_BAR_RETRY_DELAY: Duration = Duration::from_secs(1);

pub(super) fn resolve_historical_bar_start_ns(
    start_ns: Option<UnixNanos>,
    now_ns: UnixNanos,
) -> UnixNanos {
    start_ns.unwrap_or(now_ns)
}

pub(super) fn resolve_historical_bar_replay_start_ns(
    first_start_ns: UnixNanos,
    last_disconnection_ns: Option<UnixNanos>,
) -> UnixNanos {
    match last_disconnection_ns {
        Some(last_disconnection_ns) if last_disconnection_ns > first_start_ns => {
            last_disconnection_ns
        }
        _ => first_start_ns,
    }
}

pub(super) fn calculate_historical_bar_subscription_duration(
    bar_type: BarType,
    start_ns: UnixNanos,
    now_ns: UnixNanos,
) -> ibapi::market_data::historical::Duration {
    use ibapi::market_data::historical::ToDuration;

    let bar_seconds = bar_type.spec().timedelta().num_seconds().max(1);
    let requested_seconds =
        ((now_ns.as_u64().saturating_sub(start_ns.as_u64())) / 1_000_000_000) as i64;
    let minimum_seconds = bar_seconds.saturating_mul(HISTORICAL_BAR_MIN_COUNT);
    let duration_seconds = requested_seconds.max(minimum_seconds).max(bar_seconds);

    if duration_seconds >= 86_400 {
        let duration_days = ((duration_seconds + 86_399) / 86_400).min(i32::MAX as i64) as i32;
        duration_days.days()
    } else {
        let duration_seconds = duration_seconds.min(i32::MAX as i64) as i32;
        duration_seconds.seconds()
    }
}

fn should_emit_historical_bar(bar: &Bar, start_ns: UnixNanos) -> bool {
    bar.ts_init >= start_ns
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_historical_bars_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    bar_type: BarType,
    what_to_show: HistoricalWhatToShow,
    price_precision: u8,
    size_precision: u8,
    use_rth: bool,
    start_ns: Option<UnixNanos>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    _handle_revised_bars: bool,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    tracing::debug!("Starting historical bars subscription for {}", bar_type);

    let first_start_ns = resolve_historical_bar_start_ns(start_ns, clock.get_time_ns());
    let mut last_disconnection_ns = None;

    loop {
        if cancellation_token.is_cancelled() {
            break;
        }

        while !client.is_connected() {
            if cancellation_token.is_cancelled() {
                tracing::debug!("Historical bars subscription cancelled for {}", bar_type);
                return Ok(());
            }
            tokio::time::sleep(HISTORICAL_BAR_RETRY_DELAY).await;
        }

        let replay_start_ns =
            resolve_historical_bar_replay_start_ns(first_start_ns, last_disconnection_ns);
        let request_duration = calculate_historical_bar_subscription_duration(
            bar_type,
            replay_start_ns,
            clock.get_time_ns(),
        );
        let trading_hours = if use_rth {
            TradingHours::Regular
        } else {
            TradingHours::Extended
        };
        let mut subscription = match client
            .historical_data_streaming(
                &contract,
                request_duration,
                bar_type_to_historical_bar_size(bar_type)?,
                Some(what_to_show),
                trading_hours,
                true,
            )
            .await
        {
            Ok(subscription) => subscription,
            Err(e) => {
                tracing::warn!(
                    "Failed to create historical bars subscription for {}: {:?}",
                    bar_type,
                    e
                );
                last_disconnection_ns = Some(clock.get_time_ns());
                tokio::time::sleep(HISTORICAL_BAR_RETRY_DELAY).await;
                continue;
            }
        };

        loop {
            tokio::select! {
                () = cancellation_token.cancelled() => {
                    tracing::debug!("Historical bars subscription cancelled for {}", bar_type);
                    subscription.cancel().await;
                    return Ok(());
                }
                update = subscription.next() => {
                    match update {
                        Some(HistoricalBarUpdate::Historical(data)) => {
                            for ib_bar in &data.bars {
                                let bar = ib_bar_to_nautilus_bar(
                                    ib_bar,
                                    bar_type,
                                    price_precision,
                                    size_precision,
                                )?;

                                if should_emit_historical_bar(&bar, replay_start_ns)
                                    && data_sender.send(DataEvent::Data(Data::Bar(bar))).is_err()
                                {
                                    return Ok(());
                                }
                            }
                        }
                        Some(HistoricalBarUpdate::Update(ib_bar)) => {
                            let bar = ib_bar_to_nautilus_bar(
                                &ib_bar,
                                bar_type,
                                price_precision,
                                size_precision,
                            )?;

                            if should_emit_historical_bar(&bar, replay_start_ns)
                                && data_sender.send(DataEvent::Data(Data::Bar(bar))).is_err()
                            {
                                return Ok(());
                            }
                        }
                        Some(HistoricalBarUpdate::End { .. }) => {}
                        None => {
                            if cancellation_token.is_cancelled() {
                                return Ok(());
                            }

                            if let Some(error) = subscription.error() {
                                tracing::warn!(
                                    "Historical bars subscription ended for {}: {:?}",
                                    bar_type,
                                    error
                                );
                            } else {
                                tracing::warn!(
                                    "Historical bars subscription ended unexpectedly for {}",
                                    bar_type
                                );
                            }

                            last_disconnection_ns = Some(clock.get_time_ns());
                            break;
                        }
                    }
                }
            }
        }

        tokio::time::sleep(HISTORICAL_BAR_RETRY_DELAY).await;
    }

    tracing::debug!("Historical bars subscription ended for {}", bar_type);
    Ok(())
}

fn bar_type_to_historical_bar_size(bar_type: BarType) -> anyhow::Result<HistoricalBarSize> {
    crate::data::convert::bar_type_to_ib_bar_size(&bar_type)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_quote_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    quote_cache: Arc<tokio::sync::Mutex<QuoteCache>>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    ignore_size_updates: bool,
) -> anyhow::Result<()> {
    tracing::debug!("Starting quote subscription for {}", instrument_id);

    let mut subscription = client
        .market_data(&contract)
        .streaming()
        .subscribe()
        .await
        .context("Failed to create market data subscription")?;

    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::debug!("Quote subscription cancelled for {}", instrument_id);
                subscription.cancel().await;
                break;
            }
            tick_result = subscription.next() => {
                let Some(tick_result) = tick_result else {
                    tracing::debug!("Quote subscription stream ended for {}", instrument_id);
                    break;
                };

                if matches!(
                    process_quote_tick_result(
                        tick_result,
                        instrument_id,
                        price_precision,
                        size_precision,
                        &data_sender,
                        &quote_cache,
                        clock,
                        ignore_size_updates,
                    )
                    .await?,
                    StreamAction::Stop
                ) {
                    break;
                }
            }
        }
    }

    tracing::debug!("Quote subscription ended for {}", instrument_id);
    Ok(())
}

pub(super) async fn handle_option_greeks_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    option_greeks_cache: Arc<tokio::sync::Mutex<OptionGreeksCache>>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    tracing::debug!("Starting option greeks subscription for {}", instrument_id);

    let mut subscription = client
        .market_data(&contract)
        .generic_ticks(&["101"])
        .streaming()
        .subscribe()
        .await
        .context("Failed to create option greeks market data subscription")?;

    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::debug!("Option greeks subscription cancelled for {}", instrument_id);
                subscription.cancel().await;
                break;
            }
            tick_result = subscription.next() => {
                let Some(tick_result) = tick_result else {
                    tracing::debug!("Option greeks subscription stream ended for {}", instrument_id);
                    break;
                };

                if matches!(
                    process_option_greeks_tick_result(
                        tick_result,
                        instrument_id,
                        &data_sender,
                        &option_greeks_cache,
                        clock,
                    )
                    .await?,
                    StreamAction::Stop
                ) {
                    break;
                }
            }
        }
    }

    tracing::debug!("Option greeks subscription ended for {}", instrument_id);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_index_price_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    price_magnifier: i32,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    tracing::debug!("Starting index price subscription for {}", instrument_id);

    let mut subscription = client
        .market_data(&contract)
        .streaming()
        .subscribe()
        .await
        .context("Failed to create index market data subscription")?;

    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::debug!("Index price subscription cancelled for {}", instrument_id);
                subscription.cancel().await;
                break;
            }
            tick_result = subscription.next() => {
                let Some(tick_result) = tick_result else {
                    tracing::debug!("Index price subscription stream ended for {}", instrument_id);
                    break;
                };

                if matches!(
                    process_index_price_tick_result(
                        tick_result,
                        instrument_id,
                        price_precision,
                        price_magnifier,
                        &data_sender,
                        clock,
                    )
                    .await?,
                    StreamAction::Stop
                ) {
                    break;
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_tick_by_tick_quote_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    price_magnifier: f64,
) -> anyhow::Result<()> {
    tracing::debug!(
        "Starting tick-by-tick quote subscription for {}",
        instrument_id
    );

    let mut subscription = client
        .tick_by_tick_bid_ask(&contract, 0, false)
        .await
        .context("Failed to create tick-by-tick bid/ask subscription")?;

    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::debug!("Tick-by-tick quote subscription cancelled for {}", instrument_id);
                subscription.cancel().await;
                break;
            }
            tick_result = subscription.next() => {
                match tick_result {
                    Some(Ok(bid_ask)) => {
                        let ts_event = ib_timestamp_to_unix_nanos(&bid_ask.time);
                        let ts_init = clock.get_time_ns();

                        let bid_price = bid_ask.bid_price * price_magnifier;
                        let ask_price = bid_ask.ask_price * price_magnifier;

                        match parse_quote_tick(
                            instrument_id,
                            Some(bid_price),
                            Some(ask_price),
                            Some(bid_ask.bid_size),
                            Some(bid_ask.ask_size),
                            price_precision,
                            size_precision,
                            ts_event,
                            ts_init,
                        ) {
                            Ok(quote_tick) => {
                                if data_sender
                                    .send(DataEvent::Data(Data::Quote(quote_tick)))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(e) => tracing::warn!("Failed to parse quote tick: {:?}", e),
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("Subscription error for {}: {:?}", instrument_id, e);
                        anyhow::bail!("Subscription error: {e:?}");
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_trade_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    tracing::debug!("Starting trade subscription for {}", instrument_id);

    let mut subscription = client
        .tick_by_tick_all_last(&contract, 0, false)
        .await
        .context("Failed to create tick-by-tick trade subscription")?;

    process_trade_stream(
        &mut subscription,
        instrument_id,
        price_precision,
        size_precision,
        &data_sender,
        clock,
        &cancellation_token,
    )
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_realtime_bars_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    bar_type: BarType,
    bar_type_str: String,
    _instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    _clock: &'static AtomicTime,
    last_bars: Arc<tokio::sync::Mutex<AHashMap<String, RealtimeBar>>>,
    bar_timeout_tasks: Arc<tokio::sync::Mutex<AHashMap<String, tokio::task::JoinHandle<()>>>>,
    handle_revised_bars: bool,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    tracing::debug!("Starting bars subscription for {}", bar_type);

    let mut subscription = client
        .realtime_bars(
            &contract,
            RealtimeBarSize::Sec5,
            RealtimeWhatToShow::Trades,
            TradingHours::Regular,
        )
        .await
        .context("Failed to create realtime bars subscription")?;

    process_realtime_bar_stream(
        &mut subscription,
        bar_type,
        &bar_type_str,
        price_precision,
        size_precision,
        &data_sender,
        &last_bars,
        &bar_timeout_tasks,
        handle_revised_bars,
        &cancellation_token,
    )
    .await?;

    Ok(())
}

async fn update_revised_bar_tracking(
    bar_type_str: &str,
    bar: RealtimeBar,
    last_bars: &Arc<tokio::sync::Mutex<AHashMap<String, RealtimeBar>>>,
    bar_timeout_tasks: &Arc<tokio::sync::Mutex<AHashMap<String, tokio::task::JoinHandle<()>>>>,
) {
    last_bars.lock().await.insert(bar_type_str.to_string(), bar);

    if let Some(existing) = bar_timeout_tasks.lock().await.remove(bar_type_str) {
        existing.abort();
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_trade_stream(
    subscription: &mut Subscription<ibapi::market_data::realtime::Trade>,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: &CancellationToken,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::debug!("Trade subscription cancelled for {}", instrument_id);
                subscription.cancel().await;
                break;
            }
            tick_result = subscription.next() => {
                match tick_result {
                    Some(Ok(tick)) => {
                        let ts_event = ib_timestamp_to_unix_nanos(&tick.time);
                        let ts_init = clock.get_time_ns();

                        match parse_trade_tick(
                            instrument_id,
                            tick.price,
                            tick.size,
                            price_precision,
                            size_precision,
                            ts_event,
                            ts_init,
                            None,
                        ) {
                            Ok(trade_tick) => {
                                if data_sender.send(DataEvent::Data(Data::Trade(trade_tick))).is_err() {
                                    break;
                                }
                            }
                            Err(e) => tracing::warn!("Failed to parse trade tick: {:?}", e),
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("Trade subscription error for {}: {:?}", instrument_id, e);
                        anyhow::bail!("Subscription error: {e:?}");
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_realtime_bar_stream(
    subscription: &mut Subscription<RealtimeBar>,
    bar_type: BarType,
    bar_type_str: &str,
    price_precision: u8,
    size_precision: u8,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    last_bars: &Arc<tokio::sync::Mutex<AHashMap<String, RealtimeBar>>>,
    bar_timeout_tasks: &Arc<tokio::sync::Mutex<AHashMap<String, tokio::task::JoinHandle<()>>>>,
    handle_revised_bars: bool,
    cancellation_token: &CancellationToken,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::debug!("Bars subscription cancelled for {}", bar_type);
                subscription.cancel().await;
                break;
            }
            bar_result = subscription.next() => {
                match bar_result {
                    Some(Ok(bar)) => {
                        let parsed_bar = ib_bar_to_nautilus_bar(
                            &HistoricalBar {
                                date: bar.date,
                                open: bar.open,
                                high: bar.high,
                                low: bar.low,
                                close: bar.close,
                                volume: bar.volume,
                                wap: bar.wap,
                                count: bar.count,
                            },
                            bar_type,
                            price_precision,
                            size_precision,
                        )?;

                        if data_sender.send(DataEvent::Data(Data::Bar(parsed_bar))).is_err() {
                            break;
                        }

                        if handle_revised_bars {
                            update_revised_bar_tracking(
                                bar_type_str,
                                bar,
                                last_bars,
                                bar_timeout_tasks,
                            )
                            .await;
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("Bars subscription error for {}: {:?}", bar_type, e);
                        anyhow::bail!("Subscription error: {e:?}");
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_market_depth_stream(
    subscription: &mut Subscription<MarketDepths>,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: &CancellationToken,
) -> anyhow::Result<()> {
    let mut sequence: u64 = 0;

    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => {
                subscription.cancel().await;
                break;
            }
            depth_result = subscription.next() => {
                match depth_result {
                    Some(Ok(MarketDepths::MarketDepth(depth))) => {
                        let ts_event = clock.get_time_ns();
                        let ts_init = ts_event;
                        sequence += 1;
                        let order_side = if depth.side == 1 { OrderSide::Buy } else { OrderSide::Sell };
                        let action = parse_market_depth_operation(depth.operation);
                        let price = Price::new(depth.price, price_precision);
                        let size = Quantity::new(depth.size, size_precision);
                        let order_id = depth.position as u64;
                        let order = BookOrder::new(order_side, price, size, order_id);
                        let delta = OrderBookDelta::new(
                            instrument_id,
                            action,
                            order,
                            0,
                            sequence,
                            ts_event,
                            ts_init,
                        );

                        if data_sender.send(DataEvent::Data(Data::Delta(delta))).is_err() {
                            break;
                        }
                    }
                    Some(Ok(MarketDepths::MarketDepthL2(depth))) => {
                        let ts_event = clock.get_time_ns();
                        let ts_init = ts_event;
                        sequence += 1;
                        let order_side = if depth.side == 1 { OrderSide::Buy } else { OrderSide::Sell };
                        let action = parse_market_depth_operation(depth.operation);
                        let price = Price::new(depth.price, price_precision);
                        let size = Quantity::new(depth.size, size_precision);
                        let order_id = (depth.position as u64 * 1000)
                            + (depth
                                .market_maker
                                .as_bytes()
                                .iter()
                                .take(3)
                                .map(|&b| b as u64)
                                .sum::<u64>()
                                % 1000);
                        let order = BookOrder::new(order_side, price, size, order_id);
                        let delta = OrderBookDelta::new(
                            instrument_id,
                            action,
                            order,
                            0,
                            sequence,
                            ts_event,
                            ts_init,
                        );

                        if data_sender.send(DataEvent::Data(Data::Delta(delta))).is_err() {
                            break;
                        }
                    }
                    Some(Ok(MarketDepths::Notice(notice))) => {
                        tracing::debug!(
                            "IB market depth notice for {}: {} - {}",
                            instrument_id,
                            notice.code,
                            notice.message
                        );
                    }
                    Some(Err(e)) => anyhow::bail!("Subscription error: {e:?}"),
                    None => break,
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_market_depth_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    depth_rows: i32,
    is_smart_depth: bool,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    let mut subscription = client
        .market_depth(&contract, depth_rows, is_smart_depth)
        .await
        .context("Failed to create market depth subscription")?;

    process_market_depth_stream(
        &mut subscription,
        instrument_id,
        price_precision,
        size_precision,
        &data_sender,
        clock,
        &cancellation_token,
    )
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_quote_tick_result<E: Debug>(
    tick_result: Result<TickTypes, E>,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    quote_cache: &Arc<tokio::sync::Mutex<QuoteCache>>,
    clock: &'static AtomicTime,
    ignore_size_updates: bool,
) -> anyhow::Result<StreamAction> {
    match tick_result {
        Ok(TickTypes::Price(price)) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let quote = {
                let mut cache = quote_cache.lock().await;
                update_quote_from_price_tick(
                    &mut cache,
                    instrument_id,
                    &price,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                )
            };

            Ok(send_quote_tick(quote, data_sender, instrument_id))
        }
        Ok(TickTypes::Size(size)) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let quote = {
                let mut cache = quote_cache.lock().await;
                update_quote_from_size_tick(
                    &mut cache,
                    instrument_id,
                    &size,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                    ignore_size_updates,
                )
            };

            Ok(send_quote_tick(quote, data_sender, instrument_id))
        }
        Ok(TickTypes::PriceSize(price_size)) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let quote = {
                let mut cache = quote_cache.lock().await;
                update_quote_from_price_size_tick(
                    &mut cache,
                    instrument_id,
                    &price_size,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                )
            };

            Ok(send_quote_tick(quote, data_sender, instrument_id))
        }
        Ok(TickTypes::Notice(notice)) => {
            tracing::debug!(
                "IB notice for {}: {} - {}",
                instrument_id,
                notice.code,
                notice.message
            );

            if notice.code == 162 {
                tracing::info!("Market data subscription cancelled for {}", instrument_id);
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(TickTypes::SnapshotEnd) => {
            tracing::debug!("Snapshot end received for {}", instrument_id);
            Ok(StreamAction::Continue)
        }
        Ok(_) => Ok(StreamAction::Continue),
        Err(e) => {
            tracing::error!("Subscription error for {}: {:?}", instrument_id, e);
            anyhow::bail!("Subscription error: {e:?}");
        }
    }
}

async fn process_option_greeks_tick_result<E: Debug>(
    tick_result: Result<TickTypes, E>,
    instrument_id: InstrumentId,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    option_greeks_cache: &Arc<tokio::sync::Mutex<OptionGreeksCache>>,
    clock: &'static AtomicTime,
) -> anyhow::Result<StreamAction> {
    match tick_result {
        Ok(TickTypes::OptionComputation(computation)) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let greeks = {
                let mut cache = option_greeks_cache.lock().await;
                cache.update_from_computation(instrument_id, &computation, ts_event, ts_init)
            };

            Ok(send_option_greeks(greeks, data_sender, instrument_id))
        }
        Ok(TickTypes::Generic(TickGeneric { tick_type, value })) => {
            process_option_open_interest_tick(
                instrument_id,
                tick_type,
                value,
                data_sender,
                option_greeks_cache,
                clock,
            )
            .await
        }
        Ok(TickTypes::Size(TickSize { tick_type, size })) => {
            process_option_open_interest_tick(
                instrument_id,
                tick_type,
                size,
                data_sender,
                option_greeks_cache,
                clock,
            )
            .await
        }
        Ok(TickTypes::Notice(notice)) => {
            tracing::debug!(
                "IB option greeks notice for {}: {} - {}",
                instrument_id,
                notice.code,
                notice.message
            );

            if notice.code == 162 {
                tracing::info!("Option greeks subscription cancelled for {}", instrument_id);
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(TickTypes::SnapshotEnd) => {
            tracing::debug!("Option greeks snapshot end received for {}", instrument_id);
            Ok(StreamAction::Continue)
        }
        Ok(_) => Ok(StreamAction::Continue),
        Err(e) => {
            tracing::error!(
                "Option greeks subscription error for {}: {:?}",
                instrument_id,
                e
            );
            anyhow::bail!("Subscription error: {e:?}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_index_price_tick_result<E: Debug>(
    tick_result: Result<TickTypes, E>,
    instrument_id: InstrumentId,
    price_precision: u8,
    price_magnifier: i32,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
) -> anyhow::Result<StreamAction> {
    match tick_result {
        Ok(TickTypes::Price(price)) if matches!(price.tick_type, TickType::Last) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;
            let index_price = parse_index_price(
                instrument_id,
                price.price,
                price_precision,
                price_magnifier,
                ts_event,
                ts_init,
            )?;

            if data_sender
                .send(DataEvent::Data(Data::IndexPriceUpdate(index_price)))
                .is_err()
            {
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(TickTypes::PriceSize(price_size))
            if matches!(price_size.price_tick_type, TickType::Last) =>
        {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;
            let index_price = parse_index_price(
                instrument_id,
                price_size.price,
                price_precision,
                price_magnifier,
                ts_event,
                ts_init,
            )?;

            if data_sender
                .send(DataEvent::Data(Data::IndexPriceUpdate(index_price)))
                .is_err()
            {
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(_) => Ok(StreamAction::Continue),
        Err(e) => {
            tracing::error!(
                "Index price subscription stream error for {}: {:?}",
                instrument_id,
                e
            );
            anyhow::bail!("Subscription error: {e:?}");
        }
    }
}

fn update_quote_from_price_tick(
    cache: &mut QuoteCache,
    instrument_id: InstrumentId,
    price: &TickPrice,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<QuoteTick> {
    match price.tick_type {
        TickType::Bid => cache.update_bid_price(
            instrument_id,
            price.price,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Ask => cache.update_ask_price(
            instrument_id,
            price.price,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Last => None,
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn update_quote_from_size_tick(
    cache: &mut QuoteCache,
    instrument_id: InstrumentId,
    size: &TickSize,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    ignore_size_updates: bool,
) -> Option<QuoteTick> {
    match size.tick_type {
        TickType::BidSize => cache.update_bid_size_with_filter(
            instrument_id,
            size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
            ignore_size_updates,
        ),
        TickType::AskSize => cache.update_ask_size_with_filter(
            instrument_id,
            size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
            ignore_size_updates,
        ),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn update_quote_from_price_size_tick(
    cache: &mut QuoteCache,
    instrument_id: InstrumentId,
    price_size: &TickPriceSize,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<QuoteTick> {
    let quote = match price_size.price_tick_type {
        TickType::Bid => cache.update_bid_price(
            instrument_id,
            price_size.price,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Ask => cache.update_ask_price(
            instrument_id,
            price_size.price,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Last => None,
        _ => None,
    };

    if price_size.size <= 0.0 {
        return quote;
    }

    match price_size.price_tick_type {
        TickType::Bid => cache.update_bid_size(
            instrument_id,
            price_size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Ask => cache.update_ask_size(
            instrument_id,
            price_size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        _ => quote,
    }
}

fn send_quote_tick(
    quote: Option<QuoteTick>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_id: InstrumentId,
) -> StreamAction {
    if let Some(quote_tick) = quote
        && data_sender
            .send(DataEvent::Data(Data::Quote(quote_tick)))
            .is_err()
    {
        tracing::warn!(
            "Data channel closed, stopping subscription for {}",
            instrument_id
        );
        return StreamAction::Stop;
    }

    StreamAction::Continue
}

fn send_option_greeks(
    greeks: Option<OptionGreeks>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_id: InstrumentId,
) -> StreamAction {
    if let Some(option_greeks) = greeks
        && data_sender
            .send(DataEvent::OptionGreeks(option_greeks))
            .is_err()
    {
        tracing::warn!(
            "Data channel closed, stopping option greeks subscription for {}",
            instrument_id
        );
        return StreamAction::Stop;
    }

    StreamAction::Continue
}

async fn process_option_open_interest_tick(
    instrument_id: InstrumentId,
    tick_type: TickType,
    value: f64,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    option_greeks_cache: &Arc<tokio::sync::Mutex<OptionGreeksCache>>,
    clock: &'static AtomicTime,
) -> anyhow::Result<StreamAction> {
    let Some(open_interest) = parse_option_open_interest(&tick_type, value) else {
        return Ok(StreamAction::Continue);
    };

    let ts_event = clock.get_time_ns();
    let ts_init = ts_event;
    let greeks = {
        let mut cache = option_greeks_cache.lock().await;
        cache.update_open_interest(instrument_id, open_interest, ts_event, ts_init)
    };

    Ok(send_option_greeks(greeks, data_sender, instrument_id))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ahash::AHashMap;
    use ibapi::{
        contracts::{OptionComputation, tick_types::TickType},
        market_data::realtime::{
            Bar as RealtimeBar, MarketDepth, MarketDepthL2, MarketDepths, TickAttribute,
            TickGeneric, TickPrice, TickPriceSize, TickSize, TickTypes, Trade, TradeAttribute,
        },
        messages::Notice,
        subscriptions::Subscription,
    };
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        data::{BarType, Data},
        identifiers::{InstrumentId, Symbol, Venue},
    };
    use rstest::rstest;
    use tokio_util::sync::CancellationToken;

    use super::{
        OptionGreeksCache, QuoteCache, StreamAction, process_index_price_tick_result,
        process_market_depth_stream, process_option_greeks_tick_result, process_quote_tick_result,
        process_realtime_bar_stream, process_trade_stream, send_quote_tick,
        update_quote_from_price_tick, update_revised_bar_tracking,
    };

    fn instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("SPX"), Venue::from("CBOE"))
    }

    fn minute_bar_type() -> BarType {
        BarType::from("SPX.CBOE-1-MINUTE-LAST-EXTERNAL")
    }

    #[rstest]
    fn test_resolve_historical_bar_start_ns_uses_current_time_when_missing() {
        let now_ns = UnixNanos::from(2_000);

        let start_ns = super::resolve_historical_bar_start_ns(None, now_ns);

        assert_eq!(start_ns, now_ns);
    }

    #[rstest]
    fn test_resolve_historical_bar_replay_start_ns_uses_last_disconnect_when_later() {
        let first_start_ns = UnixNanos::from(1_000);
        let last_disconnection_ns = UnixNanos::from(1_500);

        let replay_start_ns = super::resolve_historical_bar_replay_start_ns(
            first_start_ns,
            Some(last_disconnection_ns),
        );

        assert_eq!(replay_start_ns, last_disconnection_ns);
    }

    #[rstest]
    fn test_calculate_historical_bar_subscription_duration_requests_at_least_300_bars() {
        use ibapi::market_data::historical::ToDuration;

        let duration = super::calculate_historical_bar_subscription_duration(
            minute_bar_type(),
            UnixNanos::from(9_000_000_000),
            UnixNanos::from(10_000_000_000),
        );

        assert_eq!(duration, 18_000.seconds());
    }

    #[tokio::test]
    async fn test_process_index_price_tick_result_emits_index_update_from_last_price() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();

        let action = process_index_price_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Last,
                price: 452525.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            100,
            &sender,
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Continue));

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::IndexPriceUpdate(index)) => {
                assert_eq!(index.value.as_f64(), 4525.25);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_emits_quote_after_bid_and_ask_prices() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        let bid_action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Bid,
                price: 100.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();
        let ask_action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Ask,
                price: 101.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        assert!(matches!(bid_action, StreamAction::Continue));
        assert!(matches!(ask_action, StreamAction::Continue));

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.bid_price.as_f64(), 100.0);
                assert_eq!(quote.ask_price.as_f64(), 101.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_emits_quote_from_price_size_tick() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::PriceSize(TickPriceSize {
                price_tick_type: TickType::Bid,
                price: 99.5,
                attributes: TickAttribute::default(),
                size_tick_type: TickType::BidSize,
                size: 7.0,
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();
        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::PriceSize(TickPriceSize {
                price_tick_type: TickType::Ask,
                price: 100.5,
                attributes: TickAttribute::default(),
                size_tick_type: TickType::AskSize,
                size: 9.0,
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.bid_price.as_f64(), 99.5);
                assert_eq!(quote.bid_size.as_f64(), 7.0);
                assert_eq!(quote.ask_price.as_f64(), 100.5);
                assert_eq!(quote.ask_size.as_f64(), 9.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_size_only_update_respects_filter() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Bid,
                price: 100.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();
        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Ask,
                price: 101.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();

        let initial = receiver.recv().await.unwrap();
        match initial {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.bid_size.as_f64(), 0.0);
                assert_eq!(quote.ask_size.as_f64(), 0.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Size(TickSize {
                tick_type: TickType::BidSize,
                size: 12.0,
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_notice_162_stops() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        let action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Notice(Notice {
                code: 162,
                message: String::from("Market data subscription cancelled"),
                error_time: None,
            })),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Stop));
    }

    #[tokio::test]
    async fn test_process_index_price_tick_result_ignores_non_last_ticks() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();

        let action = process_index_price_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Bid,
                price: 4500.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            1,
            &sender,
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_bubbles_subscription_error() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        let result = process_quote_tick_result(
            Err::<TickTypes, _>("boom"),
            instrument_id,
            2,
            0,
            &sender,
            &quote_cache,
            clock,
            false,
        )
        .await;

        let error = result.err().unwrap();
        assert_eq!(error.to_string(), "Subscription error: \"boom\"");
    }

    #[tokio::test]
    async fn test_process_option_greeks_tick_result_merges_partial_ticks() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let greeks_cache = Arc::new(tokio::sync::Mutex::new(OptionGreeksCache::new()));

        let bid_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::OptionComputation(OptionComputation {
                field: TickType::BidOption,
                implied_volatility: Some(0.24),
                underlying_price: Some(155.0),
                ..Default::default()
            })),
            instrument_id,
            &sender,
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();
        assert!(matches!(bid_action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());

        let ask_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::OptionComputation(OptionComputation {
                field: TickType::AskOption,
                implied_volatility: Some(0.26),
                underlying_price: Some(155.0),
                ..Default::default()
            })),
            instrument_id,
            &sender,
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();
        assert!(matches!(ask_action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());

        let oi_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::Generic(TickGeneric {
                tick_type: TickType::OptionCallOpenInterest,
                value: 1000.0,
            })),
            instrument_id,
            &sender,
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();
        assert!(matches!(oi_action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());

        let model_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::OptionComputation(OptionComputation {
                field: TickType::ModelOption,
                implied_volatility: Some(0.25),
                delta: Some(0.55),
                gamma: Some(0.02),
                vega: Some(0.15),
                theta: Some(-0.05),
                underlying_price: Some(155.0),
                ..Default::default()
            })),
            instrument_id,
            &sender,
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(model_action, StreamAction::Continue));

        match receiver.recv().await.unwrap() {
            DataEvent::OptionGreeks(greeks) => {
                assert_eq!(greeks.delta, 0.55);
                assert_eq!(greeks.gamma, 0.02);
                assert_eq!(greeks.vega, 0.15);
                assert_eq!(greeks.theta, -0.05);
                assert_eq!(greeks.rho, 0.0);
                assert_eq!(greeks.mark_iv, Some(0.25));
                assert_eq!(greeks.bid_iv, Some(0.24));
                assert_eq!(greeks.ask_iv, Some(0.26));
                assert_eq!(greeks.underlying_price, Some(155.0));
                assert_eq!(greeks.open_interest, Some(1000.0));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_option_greeks_tick_result_notice_162_stops() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let greeks_cache = Arc::new(tokio::sync::Mutex::new(OptionGreeksCache::new()));

        let action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::Notice(Notice {
                code: 162,
                message: String::from("Market data subscription cancelled"),
                error_time: None,
            })),
            instrument_id,
            &sender,
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Stop));
    }

    #[tokio::test]
    async fn test_update_revised_bar_tracking_replaces_bar_and_clears_timeout_task() {
        let bar_type = String::from("AAPL.SMART-5-SECOND-LAST-EXTERNAL");
        let last_bars = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));
        let bar_timeout_tasks = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));

        let stale_task = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });

        bar_timeout_tasks
            .lock()
            .await
            .insert(bar_type.clone(), stale_task);

        let bar = RealtimeBar {
            date: time::OffsetDateTime::UNIX_EPOCH,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 10.0,
            wap: 100.25,
            count: 2,
        };

        update_revised_bar_tracking(&bar_type, bar, &last_bars, &bar_timeout_tasks).await;
        tokio::task::yield_now().await;

        let last_bars_guard = last_bars.lock().await;
        let stored_bar = last_bars_guard.get(&bar_type).unwrap();
        assert_eq!(stored_bar.close, 100.5);
        assert!(!bar_timeout_tasks.lock().await.contains_key(&bar_type));
    }

    #[tokio::test]
    async fn test_process_trade_stream_emits_trade_tick() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (trade_sender, trade_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = Subscription::new(trade_receiver);
        let cancellation_token = CancellationToken::new();
        let clock = get_atomic_clock_realtime();

        trade_sender
            .send(Ok(Trade {
                tick_type: String::from("Last"),
                time: time::OffsetDateTime::UNIX_EPOCH,
                price: 4500.25,
                size: 3.0,
                trade_attribute: TradeAttribute {
                    past_limit: false,
                    unreported: false,
                },
                exchange: String::from("CBOE"),
                special_conditions: String::new(),
            }))
            .unwrap();
        drop(trade_sender);

        process_trade_stream(
            &mut subscription,
            instrument_id,
            2,
            0,
            &sender,
            clock,
            &cancellation_token,
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Trade(trade)) => {
                assert_eq!(trade.price.as_f64(), 4500.25);
                assert_eq!(trade.size.as_f64(), 3.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_realtime_bar_stream_emits_bar_and_tracks_revision() {
        let bar_type = BarType::from("SPX.CBOE-5-SECOND-LAST-EXTERNAL");
        let bar_type_str = bar_type.to_string();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (bar_sender, bar_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = Subscription::new(bar_receiver);
        let cancellation_token = CancellationToken::new();
        let last_bars = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));
        let timeout_tasks = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));

        bar_sender
            .send(Ok(RealtimeBar {
                date: time::OffsetDateTime::UNIX_EPOCH,
                open: 100.0,
                high: 101.0,
                low: 99.5,
                close: 100.5,
                volume: 10.0,
                wap: 100.25,
                count: 2,
            }))
            .unwrap();
        drop(bar_sender);

        process_realtime_bar_stream(
            &mut subscription,
            bar_type,
            &bar_type_str,
            2,
            0,
            &sender,
            &last_bars,
            &timeout_tasks,
            true,
            &cancellation_token,
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Bar(bar)) => {
                assert_eq!(bar.close.as_f64(), 100.5);
                assert_eq!(bar.volume.as_f64(), 10.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(last_bars.lock().await.contains_key(&bar_type_str));
    }

    #[tokio::test]
    async fn test_process_market_depth_stream_emits_deltas_with_sequence() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (depth_sender, depth_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = Subscription::new(depth_receiver);
        let cancellation_token = CancellationToken::new();
        let clock = get_atomic_clock_realtime();

        depth_sender
            .send(Ok(MarketDepths::MarketDepth(MarketDepth {
                position: 1,
                operation: 0,
                side: 1,
                price: 100.0,
                size: 5.0,
            })))
            .unwrap();
        depth_sender
            .send(Ok(MarketDepths::MarketDepthL2(MarketDepthL2 {
                position: 2,
                market_maker: String::from("MM1"),
                operation: 1,
                side: 0,
                price: 101.0,
                size: 7.0,
                smart_depth: true,
            })))
            .unwrap();
        drop(depth_sender);

        process_market_depth_stream(
            &mut subscription,
            instrument_id,
            2,
            0,
            &sender,
            clock,
            &cancellation_token,
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Delta(delta)) => {
                assert_eq!(delta.sequence, 1);
                assert_eq!(delta.order.price.as_f64(), 100.0);
                assert_eq!(delta.order.size.as_f64(), 5.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Delta(delta)) => {
                assert_eq!(delta.sequence, 2);
                assert_eq!(delta.order.price.as_f64(), 101.0);
                assert_eq!(delta.order.size.as_f64(), 7.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[rstest]
    fn test_send_quote_tick_returns_continue_for_none() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        assert!(matches!(
            send_quote_tick(None, &sender, instrument_id),
            StreamAction::Continue
        ));
    }

    #[rstest]
    fn test_update_quote_from_price_tick_ignores_last() {
        let instrument_id = instrument_id();
        let mut cache = QuoteCache::new();
        let quote = update_quote_from_price_tick(
            &mut cache,
            instrument_id,
            &TickPrice {
                tick_type: TickType::Last,
                price: 100.0,
                attributes: TickAttribute::default(),
            },
            2,
            0,
            nautilus_core::UnixNanos::new(1),
            nautilus_core::UnixNanos::new(1),
        );
        assert!(quote.is_none());
    }
}
