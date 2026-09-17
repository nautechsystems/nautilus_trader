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

//! Parsing of Kalshi responses into Nautilus domain types.
//!
//! Every price and contract count is parsed from the exchange's fixed-point strings into exact
//! decimals. No value in this module passes through a floating point representation, because a
//! sub-cent price or a fractional contract would lose its value.

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, InstrumentClose, InstrumentStatus, OrderBookDelta, QuoteTick, TradeTick},
    enums::{
        AggressorSide, AssetClass, BookAction, InstrumentCloseType, MarketStatusAction, OrderSide,
    },
    identifiers::{InstrumentId, OutcomeGroupId, Symbol, TradeId, Venue},
    instruments::{InstrumentAny, binary_option::BinaryOption},
    prediction::{
        Exclusivity, Exhaustiveness, MarketResolution, OutcomeGroup, OutcomeLeg, OutcomePayout,
        ResolutionOutcome, ResolutionSource,
    },
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::{KALSHI_CURRENCY, KALSHI_PRICE_PRECISION, KALSHI_SIZE_PRECISION, KALSHI_VENUE},
        enums::{KalshiMarketStatus, KalshiMarketType},
    },
    http::{
        error::{Error, Result},
        models::{KalshiEvent, KalshiHistoricalCutoff, KalshiMarket, KalshiOrderbook, KalshiTrade},
    },
};

/// The decimal places a price grid step carries.
fn step_precision(step: &Decimal) -> Result<u8> {
    let normalized = step.normalize();

    if normalized.scale() > u32::from(KALSHI_PRICE_PRECISION) {
        return Err(Error::Serde(format!(
            "Kalshi price step {step} exceeds {KALSHI_PRICE_PRECISION} decimal places"
        )));
    }

    u8::try_from(normalized.scale()).map_err(|e| {
        Error::Serde(format!(
            "Kalshi price step {step} has no usable precision: {e}"
        ))
    })
}

/// Returns the exact decimal value of a Kalshi numeric string.
fn parse_decimal(value: &str, field: &str) -> Result<Decimal> {
    Decimal::from_str_exact(value.trim()).map_err(|e| {
        Error::Serde(format!(
            "Failed to parse Kalshi {field} value '{value}' as decimal: {e}"
        ))
    })
}

/// Returns the instrument identifier for a Kalshi market ticker.
#[must_use]
pub fn instrument_id_for(ticker: &str) -> InstrumentId {
    InstrumentId::from(format!("{ticker}.{KALSHI_VENUE}").as_str())
}

/// Parses an RFC 3339 timestamp into Unix nanoseconds.
///
/// # Errors
///
/// Returns an error if the value is not an RFC 3339 timestamp.
pub fn parse_datetime_to_nanos(value: &str, field: &str) -> Result<UnixNanos> {
    let timestamp = jiff::Timestamp::from_str(value.trim()).map_err(|e| {
        Error::Serde(format!(
            "Failed to parse Kalshi {field} timestamp '{value}': {e}"
        ))
    })?;
    let nanos = timestamp.as_nanosecond();

    u64::try_from(nanos).map(UnixNanos::from).map_err(|_| {
        Error::Serde(format!(
            "Kalshi {field} timestamp '{value}' is out of range"
        ))
    })
}

/// Parses a fixed-point dollar string into a [`Price`] at the given precision.
///
/// An empty field means the exchange has no quote on that side, which is not an error for callers
/// that tolerate one-sided markets only when they ask for it: this function reports it.
///
/// # Errors
///
/// Returns an error if the value is empty or not a fixed-point dollar string.
pub fn parse_price_dollars(value: &str, precision: u8, field: &str) -> Result<Price> {
    if value.trim().is_empty() {
        return Err(Error::Serde(format!("Kalshi {field} price is empty")));
    }

    let decimal = parse_decimal(value, field)?;

    Price::from_decimal_dp(decimal, precision)
        .map_err(|e| Error::Serde(format!("Failed to build price from {field} '{value}': {e}")))
}

/// Parses a fixed-point contract count into a [`Quantity`] at the given precision.
///
/// # Errors
///
/// Returns an error if the value is empty or not a fixed-point count.
pub fn parse_count_fp(value: &str, precision: u8, field: &str) -> Result<Quantity> {
    if value.trim().is_empty() {
        return Err(Error::Serde(format!("Kalshi {field} count is empty")));
    }

    let decimal = parse_decimal(value, field)?;

    Quantity::from_decimal_dp(decimal, precision).map_err(|e| {
        Error::Serde(format!(
            "Failed to build quantity from {field} '{value}': {e}"
        ))
    })
}

/// Parses a fixed-point dollar string into USD [`Money`].
///
/// Kalshi quotes fees and direct-member balances with more decimal places than the currency carries,
/// and a `Money` value is denominated in the currency's scale, so a value that cannot be represented
/// exactly is reported at its rounded amount with a warning rather than silently.
///
/// # Errors
///
/// Returns an error if the value is not a fixed-point dollar string.
pub fn parse_money_dollars(value: &str, field: &str) -> Result<Money> {
    let decimal = parse_decimal(value, field)?;
    let currency = Currency::from(KALSHI_CURRENCY);

    if decimal.normalize().scale() > u32::from(currency.precision) {
        log::warn!(
            "Kalshi {field} '{value}' is finer than {currency} represents; reporting the rounded amount"
        );
    }

    Money::from_decimal(decimal, currency)
        .map_err(|e| Error::Serde(format!("Failed to build money from {field} '{value}': {e}")))
}

/// Parses a fixed-point dollar string into a [`Decimal`].
///
/// # Errors
///
/// Returns an error if the value is not a fixed-point dollar string.
pub fn parse_dollars(value: &str, field: &str) -> Result<Decimal> {
    parse_decimal(value, field)
}

/// Returns the oldest timestamp for which the venue's live endpoints hold the full record.
///
/// Orders and fills older than the later of the two cutoffs are only available from the venue's
/// historical endpoints, so a request window starting before it is covered only in part.
#[must_use]
pub fn historical_coverage_floor(cutoff: &KalshiHistoricalCutoff) -> Option<UnixNanos> {
    let orders_updated = cutoff.orders_updated_ts.as_deref()?;
    let trades_created = cutoff.trades_created_ts.as_deref()?;
    let orders = parse_datetime_to_nanos(orders_updated, "orders_updated_ts").ok()?;
    let fills = parse_datetime_to_nanos(trades_created, "trades_created_ts").ok()?;

    Some(orders.max(fills))
}

/// Returns the quote precision and tick increment a market's price grid advertises.
///
/// The exchange publishes `price_ranges` as bands, each with its own step. The finest step is the
/// increment, because every price on the grid is a multiple of it. A tapered grid cannot be
/// expressed as one increment, so prices inside a coarser band that fall between the finer steps
/// are still rejected by the exchange.
///
/// # Errors
///
/// Returns an error if the market publishes no price range or a step that cannot be represented.
pub fn price_precision_and_increment(market: &KalshiMarket) -> Result<(u8, Price)> {
    let mut finest: Option<Decimal> = None;

    for range in &market.price_ranges {
        let step = parse_decimal(&range.step, "price_ranges.step")?;

        if step <= Decimal::ZERO {
            return Err(Error::Serde(format!(
                "Kalshi market {} has a non-positive price step '{0}'",
                market.ticker
            )));
        }

        if finest.is_none_or(|current| step < current) {
            finest = Some(step);
        }
    }

    let step = finest.ok_or_else(|| {
        Error::Serde(format!(
            "Kalshi market {} publishes no price range, so its price grid is unknown",
            market.ticker
        ))
    })?;
    let precision = step_precision(&step)?;

    Price::from_decimal_dp(step, precision)
        .map(|price| (precision, price))
        .map_err(|e| {
            Error::Serde(format!(
                "Failed to build a price increment for Kalshi market {}: {e}",
                market.ticker
            ))
        })
}

/// Converts a market into a Nautilus binary option instrument.
///
/// The instrument is the market itself: its YES and NO sides are the same contract, quoted from
/// the YES side.
///
/// # Errors
///
/// Returns an error if the market's timestamps, price grid, or notional value cannot be parsed.
pub fn create_instrument_from_market(
    market: &KalshiMarket,
    ts_init: UnixNanos,
) -> Result<InstrumentAny> {
    let instrument_id = instrument_id_for(&market.ticker);
    let activation_ns = parse_datetime_to_nanos(&market.open_time, "open_time")?;
    let expiration_ns =
        parse_datetime_to_nanos(&market.latest_expiration_time, "latest_expiration_time")?;
    let (price_precision, price_increment) = price_precision_and_increment(market)?;
    let _notional = parse_money_dollars(&market.notional_value_dollars, "notional_value_dollars")?;

    let binary_option = BinaryOption::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(market.ticker.as_str()))
        .asset_class(AssetClass::Alternative)
        .currency(Currency::from(KALSHI_CURRENCY))
        .activation_ns(activation_ns)
        .expiration_ns(expiration_ns)
        .price_precision(price_precision)
        .size_precision(KALSHI_SIZE_PRECISION)
        .price_increment(price_increment)
        .size_increment(Quantity::from("0.01"))
        .maybe_event_id(Some(Ustr::from(market.event_ticker.as_str())))
        .maybe_outcome(Some(Ustr::from(market.yes_sub_title.as_str())))
        .maybe_description(Some(Ustr::from(market.rules_primary.as_str())))
        .maybe_max_quantity(None)
        .maybe_min_quantity(None)
        .maybe_max_notional(None)
        .maybe_min_notional(None)
        .maybe_max_price(None)
        .maybe_min_price(None)
        .maybe_margin_init(None)
        .maybe_margin_maint(None)
        .maybe_maker_fee(None)
        .maybe_taker_fee(None)
        .maybe_tick_scheme(None)
        .maybe_info(None)
        .ts_event(ts_init)
        .ts_init(ts_init)
        .build()
        .map_err(|e| {
            Error::Serde(format!(
                "Failed to build a binary option for Kalshi market {}: {e}",
                market.ticker
            ))
        })?;

    Ok(InstrumentAny::BinaryOption(binary_option))
}

/// Builds the outcome group for an event's markets.
///
/// An event whose markets are mutually exclusive carries the outcomes of one occurrence, so it maps
/// onto a group whose legs are the event's markets and whose payout per leg is the market's
/// notional value. Exclusivity is claimed rather than proven: the exchange documents it, and the
/// adapter has not verified it beyond that. Exhaustiveness is unknown, because the exchange does
/// not promise that the listed markets name every possible outcome.
///
/// Returns `None` when the event cannot be expressed as a group: its markets are not mutually
/// exclusive, it carries no markets, it mixes non-binary markets, or its markets disagree on what a
/// winning contract pays.
///
/// # Errors
///
/// Returns an error if a market's notional value cannot be parsed, or if the constructed group
/// fails validation.
pub fn create_outcome_group_from_event(
    event: &KalshiEvent,
    markets: &[KalshiMarket],
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<Option<OutcomeGroup>> {
    if !event.mutually_exclusive {
        log::debug!(
            "Kalshi event {} is not mutually exclusive, so its markets are independent and no outcome group is declared",
            event.event_ticker
        );

        return Ok(None);
    }

    if markets.is_empty() {
        log::debug!(
            "Kalshi event {} has no markets, so no outcome group is declared",
            event.event_ticker
        );

        return Ok(None);
    }

    let mut legs = Vec::with_capacity(markets.len());
    let mut unit_total: Option<Money> = None;

    for market in markets {
        if market.market_type != KalshiMarketType::Binary {
            log::debug!(
                "Kalshi event {} has a {} market, so no outcome group is declared",
                event.event_ticker,
                market.market_type
            );

            return Ok(None);
        }

        let unit_payout =
            parse_money_dollars(&market.notional_value_dollars, "notional_value_dollars")?;

        match &unit_total {
            Some(total) if total != &unit_payout => {
                log::debug!(
                    "Kalshi event {} mixes contract notionals, so no outcome group is declared",
                    event.event_ticker
                );

                return Ok(None);
            }
            Some(_) => {}
            None => unit_total = Some(unit_payout),
        }

        let label = outcome_label(market);

        legs.push(OutcomeLeg::new(
            Ustr::from(label.as_str()),
            instrument_id_for(&market.ticker),
            unit_payout,
        ));
    }

    let Some(unit_total) = unit_total else {
        // Unreachable while the early return above guards the empty case, and kept total so no
        // code path can panic on a caller's input.
        return Ok(None);
    };
    let group = OutcomeGroup::new_checked(
        group_id_for(event)?,
        Some(event.event_ticker.clone()),
        legs,
        Exclusivity::Claimed,
        Exhaustiveness::Unknown,
        unit_total,
        1,
        Some(Ustr::from("kalshi_event")),
        ts_event,
        ts_init,
    )
    .map_err(|e| {
        Error::Serde(format!(
            "Failed to build an outcome group for Kalshi event {}: {e}",
            event.event_ticker
        ))
    })?;

    Ok(Some(group))
}

/// Returns the outcome label for a market, preferring the YES side's title.
fn outcome_label(market: &KalshiMarket) -> String {
    if market.yes_sub_title.trim().is_empty() {
        market.ticker.clone()
    } else {
        market.yes_sub_title.clone()
    }
}

/// Builds the resolution an event's settled markets declare.
///
/// Returns `None` while any market is undetermined or its venue effective time is unpublished.
/// A market the exchange has marked disputed resolves to [`ResolutionOutcome::Disputed`], which
/// callers must not apply automatically.
///
/// # Errors
///
/// Returns an error if a settled market's payout or effective time cannot be parsed.
pub fn create_resolution_from_event(
    event: &KalshiEvent,
    markets: &[KalshiMarket],
    version: u32,
    ts_init: UnixNanos,
) -> Result<Option<MarketResolution>> {
    if markets.is_empty() {
        return Ok(None);
    }

    if markets.iter().any(|market| market.status.is_disputed()) {
        let effective_ns = markets
            .iter()
            .filter_map(|market| market.settlement_ts.as_deref())
            .filter_map(|ts| parse_datetime_to_nanos(ts, "settlement_ts").ok())
            .max()
            .unwrap_or(ts_init);

        return Ok(Some(MarketResolution {
            group_id: group_id_for(event)?,
            version,
            source: resolution_source(event),
            outcome: ResolutionOutcome::Disputed,
            effective_ns,
            observed_ns: ts_init,
            ts_event: effective_ns,
            ts_init,
        }));
    }

    let mut payouts = Vec::with_capacity(markets.len());
    let mut effective_ns: Option<UnixNanos> = None;

    for market in markets {
        // Only a settled market has an outcome that took effect at a venue timestamp. A determined
        // market has not settled yet, so nothing is payable.
        let Some(settlement_ts) = market.settlement_ts.as_deref() else {
            return Ok(None);
        };

        if !market.status.is_final() || !market.result.is_binary_outcome() {
            return Ok(None);
        }

        let settled_ns = parse_datetime_to_nanos(settlement_ts, "settlement_ts")?;
        effective_ns = Some(effective_ns.map_or(settled_ns, |current| current.max(settled_ns)));

        let payout = if market.result.is_yes() {
            parse_money_dollars(&market.notional_value_dollars, "notional_value_dollars")?
        } else {
            Money::from_decimal(Decimal::ZERO, Currency::from(KALSHI_CURRENCY)).map_err(|e| {
                Error::Serde(format!(
                    "Failed to build a zero payout for Kalshi market {}: {e}",
                    market.ticker
                ))
            })?
        };

        payouts.push(OutcomePayout::new(
            Ustr::from(outcome_label(market).as_str()),
            payout,
        ));
    }

    let Some(effective_ns) = effective_ns else {
        return Ok(None);
    };

    // The exchange's own settlement time is the effective time: a resolution is only built from
    // markets the exchange has settled, so no caller-observed time is substituted for it.
    let resolution = MarketResolution {
        group_id: group_id_for(event)?,
        version,
        source: resolution_source(event),
        outcome: ResolutionOutcome::Payouts(payouts),
        effective_ns,
        observed_ns: ts_init,
        ts_event: effective_ns,
        ts_init,
    };

    Ok(Some(resolution))
}

fn group_id_for(event: &KalshiEvent) -> Result<OutcomeGroupId> {
    OutcomeGroupId::from_parts(Venue::from(KALSHI_VENUE), &event.event_ticker).map_err(|e| {
        Error::Serde(format!(
            "Invalid outcome group id for Kalshi event {}: {e}",
            event.event_ticker
        ))
    })
}

fn resolution_source(event: &KalshiEvent) -> ResolutionSource {
    // Kalshi publishes no per-outcome reference document, so the event ticker is the reference.
    ResolutionSource::new(Venue::from(KALSHI_VENUE), &event.event_ticker, None)
}

/// Builds the instrument close for a settled market.
///
/// # Errors
///
/// Returns an error if the market's settlement value or timestamp cannot be parsed.
pub fn create_instrument_close_from_market(
    market: &KalshiMarket,
    ts_init: UnixNanos,
) -> Result<InstrumentClose> {
    let close_price = match market.settlement_value_dollars.as_deref() {
        Some(value) if !value.trim().is_empty() => {
            let (precision, _) = price_precision_and_increment(market)?;

            parse_price_dollars(value, precision, "settlement_value_dollars")?
        }
        _ => {
            let (precision, _) = price_precision_and_increment(market)?;
            let payout = if market.result.is_yes() {
                market.notional_value_dollars.clone()
            } else {
                "0.00".to_string()
            };

            parse_price_dollars(&payout, precision, "expiration_value")?
        }
    };

    let ts_event = match market.settlement_ts.as_deref() {
        Some(value) => parse_datetime_to_nanos(value, "settlement_ts")?,
        None => ts_init,
    };

    Ok(InstrumentClose::new(
        instrument_id_for(&market.ticker),
        close_price,
        InstrumentCloseType::ContractExpired,
        ts_event,
        ts_init,
    ))
}

/// Builds a quote tick from a market's top of book.
///
/// Kalshi quotes everything from the YES side, so the market's YES bid and YES ask are already the
/// bid and ask of the instrument.
///
/// # Errors
///
/// Returns an error if any side is unquoted or cannot be parsed.
pub fn create_quote_tick_from_market(
    market: &KalshiMarket,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<QuoteTick> {
    let instrument_id = instrument_id_for(&market.ticker);
    let (precision, _) = price_precision_and_increment(market)?;
    let bid = parse_price_dollars(&market.yes_bid_dollars, precision, "yes_bid_dollars")?;
    let ask = parse_price_dollars(&market.yes_ask_dollars, precision, "yes_ask_dollars")?;
    let bid_size = parse_count_fp(
        &market.yes_bid_size_fp,
        KALSHI_SIZE_PRECISION,
        "yes_bid_size_fp",
    )?;
    let ask_size = parse_count_fp(
        &market.yes_ask_size_fp,
        KALSHI_SIZE_PRECISION,
        "yes_ask_size_fp",
    )?;

    Ok(QuoteTick::new(
        instrument_id,
        bid,
        ask,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    ))
}

/// Builds a trade tick from a public trade.
///
/// The aggressor is taken from `taker_outcome_side`, which describes the taker's exposure: a taker
/// positioned for `yes` bought YES, so the aggressor is a buyer from this instrument's perspective.
///
/// # Errors
///
/// Returns an error if the trade's price, size, or timestamp cannot be parsed.
pub fn create_trade_tick_from_trade(
    trade: &KalshiTrade,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Result<TradeTick> {
    let instrument_id = instrument_id_for(&trade.ticker);
    let price = parse_price_dollars(
        &trade.yes_price_dollars,
        price_precision,
        "yes_price_dollars",
    )?;
    let size = parse_count_fp(&trade.count_fp, KALSHI_SIZE_PRECISION, "count_fp")?;
    let aggressor_side = match trade.taker_outcome_side {
        crate::common::enums::KalshiOutcomeSide::Yes => AggressorSide::Buy,
        crate::common::enums::KalshiOutcomeSide::No => AggressorSide::Sell,
    };
    let ts_event = parse_datetime_to_nanos(&trade.created_time, "created_time")?;

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        TradeId::new(trade.trade_id.as_str()),
        ts_event,
        ts_init,
    ))
}

/// Builds the order book deltas that replace a market's book.
///
/// The exchange publishes bids only: a NO bid at a price is a YES ask at one minus that price with
/// the same size, so the NO levels are inverted into this instrument's ask side.
///
/// # Errors
///
/// Returns an error if any level's price or size cannot be parsed.
pub fn create_order_book_deltas_from_market(
    market: &KalshiMarket,
    orderbook: &KalshiOrderbook,
    sequence: u64,
    ts_init: UnixNanos,
) -> Result<Vec<OrderBookDelta>> {
    let instrument_id = instrument_id_for(&market.ticker);
    let (precision, _) = price_precision_and_increment(market)?;
    let mut deltas =
        Vec::with_capacity(orderbook.yes_dollars.len() + orderbook.no_dollars.len() + 1);

    deltas.push(OrderBookDelta::clear(
        instrument_id,
        sequence,
        ts_init,
        ts_init,
    ));

    for (index, (price, size)) in orderbook
        .yes_dollars
        .iter()
        .chain(orderbook.no_dollars.iter())
        .enumerate()
    {
        let (side, price_value) = if index < orderbook.yes_dollars.len() {
            (
                OrderSide::Buy,
                parse_price_dollars(price, precision, "orderbook.yes_dollars")?,
            )
        } else {
            // A NO bid at p is a YES ask at 1 - p.
            let no_price = parse_decimal(price, "orderbook.no_dollars")?;
            let yes_price = Decimal::ONE - no_price;

            (
                OrderSide::Sell,
                Price::from_decimal_dp(yes_price, precision).map_err(|e| {
                    Error::Serde(format!("Failed to invert a NO bid into a YES ask: {e}"))
                })?,
            )
        };
        let size_value = parse_count_fp(size, KALSHI_SIZE_PRECISION, "orderbook.size")?;
        let order = BookOrder::new(side, price_value, size_value, index as u64 + 1);

        deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Update,
            order,
            0,
            sequence + index as u64 + 1,
            ts_init,
            ts_init,
        ));
    }

    Ok(deltas)
}

/// Builds the market status event for a market.
///
/// The exchange's lifecycle distinguishes states that the Nautilus action enum folds together, and
/// the exchange's own status travels in `reason` so nothing is lost. A disputed market reports
/// `NotAvailableForTrading` because its outcome is contested, not because trading stopped.
#[must_use]
pub fn create_market_status(
    instrument_id: InstrumentId,
    status: KalshiMarketStatus,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentStatus {
    let action = match status {
        KalshiMarketStatus::Active => MarketStatusAction::Trading,
        KalshiMarketStatus::Disputed => MarketStatusAction::NotAvailableForTrading,
        _ => MarketStatusAction::Close,
    };
    let is_trading = status.is_tradable();

    InstrumentStatus::new(
        instrument_id,
        action,
        ts_event,
        ts_init,
        Some(Ustr::from(status.as_str())),
        None,
        Some(is_trading),
        Some(is_trading),
        None,
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::http::fixtures::{
        MARKET_JSON, TS, event, market, settled_market, settled_market_without_settlement_value,
    };

    #[rstest]
    fn test_dollars_keeps_the_scale_the_venue_publishes() {
        // A direct member's balance carries more places than a cent, and the account state is built
        // from this value rather than from the cent field.
        assert_eq!(
            parse_dollars("4125.0000", "balance_dollars").unwrap(),
            dec!(4125.0000)
        );
    }

    #[rstest]
    fn test_money_reports_an_amount_the_currency_cannot_hold() {
        // Kalshi quotes fees with more decimal places than USD carries, so those amounts are reported
        // at the currency's scale rather than at a scale the type cannot represent.
        assert_eq!(
            parse_money_dollars("0.0440", "fee_cost").unwrap(),
            Money::from("0.04 USD")
        );
        assert_eq!(
            parse_money_dollars("0.10", "fee_cost").unwrap(),
            Money::from("0.10 USD")
        );
    }

    #[rstest]
    fn test_coverage_floor_is_the_later_cutoff() {
        let cutoff = KalshiHistoricalCutoff {
            market_settled_ts: None,
            trades_created_ts: Some("2025-01-01T00:00:00Z".to_string()),
            orders_updated_ts: Some("2025-02-01T00:00:00Z".to_string()),
            market_positions_last_updated_ts: None,
        };

        assert_eq!(
            historical_coverage_floor(&cutoff),
            Some(UnixNanos::from(1_738_368_000_000_000_000u64))
        );
    }

    #[rstest]
    fn test_coverage_floor_is_unknown_without_readable_cutoffs() {
        let missing = KalshiHistoricalCutoff {
            trades_created_ts: Some("2025-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let unreadable = KalshiHistoricalCutoff {
            trades_created_ts: Some("not a timestamp".to_string()),
            orders_updated_ts: Some("2025-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };

        // Coverage is only claimed from a cutoff that can be read: without one, the window is
        // unproven rather than covered.
        assert!(historical_coverage_floor(&missing).is_none());
        assert!(historical_coverage_floor(&unreadable).is_none());
    }

    #[rstest]
    fn test_datetime_parsing_is_exact_and_rejects_garbage() {
        let nanos = parse_datetime_to_nanos("2025-01-01T00:00:00Z", "open_time").unwrap();

        assert_eq!(nanos.as_u64(), 1_735_689_600_000_000_000);
        assert!(parse_datetime_to_nanos("not a time", "open_time").is_err());
    }

    #[rstest]
    fn test_fixed_point_parsing_keeps_sub_cent_precision() {
        let price = parse_price_dollars("0.1234", 4, "yes_bid_dollars").unwrap();
        let size = parse_count_fp("1.55", 2, "count_fp").unwrap();

        assert_eq!(price, Price::from("0.1234"));
        assert_eq!(size, Quantity::from("1.55"));
    }

    #[rstest]
    fn test_empty_price_is_reported_rather_than_read_as_zero() {
        let error = parse_price_dollars("", 4, "yes_bid_dollars").unwrap_err();

        assert!(error.to_string().contains("is empty"), "{error}");
    }

    #[rstest]
    fn test_price_precision_and_increment_follow_the_finest_step() {
        let mut market = market();
        market.price_ranges = serde_json::from_str(
            r#"[{"start": "0.0000", "end": "0.1000", "step": "0.0010"},
                {"start": "0.1000", "end": "0.9000", "step": "0.0100"}]"#,
        )
        .unwrap();

        let (precision, increment) = price_precision_and_increment(&market).unwrap();

        assert_eq!(precision, 3);
        assert_eq!(increment, Price::from("0.001"));
    }

    #[rstest]
    fn test_market_without_a_price_range_is_rejected() {
        let mut market = market();
        market.price_ranges = Vec::new();

        let error = price_precision_and_increment(&market).unwrap_err();

        assert!(
            error.to_string().contains("price grid is unknown"),
            "{error}"
        );
    }

    #[rstest]
    fn test_instrument_round_trips_the_market_identity() {
        let instrument = create_instrument_from_market(&market(), UnixNanos::from(TS)).unwrap();
        let InstrumentAny::BinaryOption(option) = instrument else {
            panic!("expected a binary option");
        };

        assert_eq!(option.id, InstrumentId::from("KXHIGHNY-25JAN01-T50.KALSHI"));
        assert_eq!(option.raw_symbol.to_string(), "KXHIGHNY-25JAN01-T50");
        assert_eq!(option.event_id, Some(Ustr::from("KXHIGHNY-25JAN01")));
        assert_eq!(option.outcome, Some(Ustr::from("50 degrees or above")));
        assert_eq!(option.price_precision, 2);
        assert_eq!(option.price_increment, Price::from("0.01"));
        assert_eq!(option.size_precision, KALSHI_SIZE_PRECISION);
        assert_eq!(option.size_increment, Quantity::from("0.01"));
        assert_eq!(option.currency, Currency::from("USD"));
        assert_eq!(
            option.activation_ns,
            parse_datetime_to_nanos("2024-12-30T15:00:00Z", "open_time").unwrap()
        );
    }

    #[rstest]
    fn test_outcome_group_legs_are_the_event_markets() {
        let market = market();
        let group = create_outcome_group_from_event(
            &event(),
            std::slice::from_ref(&market),
            UnixNanos::from(TS),
            UnixNanos::from(TS),
        )
        .unwrap()
        .expect("mutually exclusive events declare a group");

        assert_eq!(group.legs.len(), 1);
        assert_eq!(
            group.legs[0].instrument_id,
            instrument_id_for(&market.ticker)
        );
        assert_eq!(group.legs[0].outcome_id, Ustr::from("50 degrees or above"));
        assert_eq!(
            group.legs[0].unit_payout,
            Money::from_decimal(dec!(1), Currency::from("USD")).unwrap()
        );
        assert_eq!(group.exclusivity, Exclusivity::Claimed);
        assert_eq!(group.exhaustiveness, Exhaustiveness::Unknown);
        assert_eq!(group.event_id.as_deref(), Some("KXHIGHNY-25JAN01"));
    }

    #[rstest]
    fn test_non_exclusive_event_declares_no_group() {
        let mut event = event();
        event.mutually_exclusive = false;

        let group = create_outcome_group_from_event(
            &event,
            &[market()],
            UnixNanos::from(TS),
            UnixNanos::from(TS),
        )
        .unwrap();

        assert!(group.is_none());
    }

    #[rstest]
    fn test_event_with_mixed_notionals_declares_no_group() {
        let mut second = market();
        second.ticker = "KXHIGHNY-25JAN01-T60".to_string();
        second.yes_sub_title = "60 degrees or above".to_string();
        second.notional_value_dollars = "2.0000".to_string();

        let group = create_outcome_group_from_event(
            &event(),
            &[market(), second],
            UnixNanos::from(TS),
            UnixNanos::from(TS),
        )
        .unwrap();

        assert!(group.is_none());
    }

    #[rstest]
    fn test_resolution_pays_the_winner_and_zeroes_the_loser() {
        let winner = settled_market("yes");
        let mut loser = settled_market("no");
        loser.ticker = "KXHIGHNY-25JAN01-T60".to_string();
        loser.yes_sub_title = "60 degrees or above".to_string();

        // The loser is a different market, so its payout is keyed by its own label.
        let resolution =
            create_resolution_from_event(&event(), &[winner, loser], 1, UnixNanos::from(TS))
                .unwrap()
                .expect("settled markets produce a resolution");

        let ResolutionOutcome::Payouts(payouts) = &resolution.outcome else {
            panic!("expected payouts, was {:?}", resolution.outcome);
        };

        assert_eq!(payouts.len(), 2);
        assert_eq!(payouts[0].outcome_id, Ustr::from("50 degrees or above"));
        assert_eq!(
            payouts[0].payout_per_unit,
            Money::from_decimal(dec!(1), Currency::from("USD")).unwrap()
        );
        assert_eq!(payouts[1].outcome_id, Ustr::from("60 degrees or above"));
        assert_eq!(
            payouts[1].payout_per_unit,
            Money::from_decimal(Decimal::ZERO, Currency::from("USD")).unwrap()
        );
        assert_eq!(resolution.version, 1);
        assert_eq!(
            resolution.effective_ns,
            parse_datetime_to_nanos("2025-01-02T06:00:00Z", "settlement_ts").unwrap()
        );
        assert_eq!(resolution.effective_ns, resolution.ts_event);
    }

    #[rstest]
    fn test_disputed_market_produces_a_disputed_resolution() {
        let raw = MARKET_JSON
            .replace("\"status\": \"active\"", "\"status\": \"disputed\"")
            .replace("\"result\": \"\"", "\"result\": \"yes\"");
        let disputed: KalshiMarket = serde_json::from_str(&raw).unwrap();

        let resolution =
            create_resolution_from_event(&event(), &[disputed], 1, UnixNanos::from(TS))
                .unwrap()
                .expect("a dispute is reported");

        assert_eq!(resolution.outcome, ResolutionOutcome::Disputed);
    }

    #[rstest]
    fn test_undetermined_market_produces_no_resolution() {
        let resolution =
            create_resolution_from_event(&event(), &[market()], 1, UnixNanos::from(TS)).unwrap();

        assert!(resolution.is_none());
    }

    #[rstest]
    fn test_determined_but_unsettled_market_produces_no_resolution() {
        let raw = MARKET_JSON
            .replace("\"status\": \"active\"", "\"status\": \"determined\"")
            .replace("\"result\": \"\"", "\"result\": \"yes\"");
        let determined: KalshiMarket = serde_json::from_str(&raw).unwrap();

        let resolution =
            create_resolution_from_event(&event(), &[determined], 1, UnixNanos::from(TS)).unwrap();

        assert!(
            resolution.is_none(),
            "a determination without a venue settlement time is not payable yet"
        );
    }

    #[rstest]
    fn test_instrument_close_uses_the_settlement_value() {
        let close =
            create_instrument_close_from_market(&settled_market("yes"), UnixNanos::from(TS))
                .unwrap();

        assert_eq!(
            close.instrument_id,
            InstrumentId::from("KXHIGHNY-25JAN01-T50.KALSHI")
        );
        assert_eq!(close.close_price, Price::from("1.00"));
        assert_eq!(close.close_type, InstrumentCloseType::ContractExpired);
        assert_eq!(
            close.ts_event,
            parse_datetime_to_nanos("2025-01-02T06:00:00Z", "settlement_ts").unwrap()
        );
    }

    #[rstest]
    fn test_instrument_close_falls_back_to_the_result_when_no_settlement_value_is_published() {
        let losing = create_instrument_close_from_market(
            &settled_market_without_settlement_value("no"),
            UnixNanos::from(TS),
        )
        .unwrap();
        let winning = create_instrument_close_from_market(
            &settled_market_without_settlement_value("yes"),
            UnixNanos::from(TS),
        )
        .unwrap();

        assert_eq!(losing.close_price, Price::from("0.00"));
        assert_eq!(winning.close_price, Price::from("1.00"));
    }

    #[rstest]
    fn test_quote_tick_reads_the_yes_side_of_the_market() {
        let quote =
            create_quote_tick_from_market(&market(), UnixNanos::from(TS), UnixNanos::from(TS))
                .unwrap();

        assert_eq!(
            quote.instrument_id,
            InstrumentId::from("KXHIGHNY-25JAN01-T50.KALSHI")
        );
        assert_eq!(quote.bid_price, Price::from("0.34"));
        assert_eq!(quote.ask_price, Price::from("0.35"));
        assert_eq!(quote.bid_size, Quantity::from("120.00"));
        assert_eq!(quote.ask_size, Quantity::from("80.00"));
    }

    #[rstest]
    fn test_trade_tick_maps_the_taker_outcome_to_an_aggressor() {
        let raw = r#"{
            "trade_id": "9f2b2b0e-1c1a-4b0e-9f7a-2b6a5b1c9d10",
            "ticker": "KXHIGHNY-25JAN01-T50",
            "count_fp": "25.00",
            "yes_price_dollars": "0.3500",
            "no_price_dollars": "0.6500",
            "taker_outcome_side": "yes",
            "created_time": "2025-01-01T12:00:00Z",
            "is_block_trade": false
        }"#;
        let trade: KalshiTrade = serde_json::from_str(raw).unwrap();

        let tick = create_trade_tick_from_trade(&trade, 2, UnixNanos::from(TS)).unwrap();

        assert_eq!(tick.aggressor_side, AggressorSide::Buy);
        assert_eq!(tick.price, Price::from("0.35"));
        assert_eq!(tick.size, Quantity::from("25.00"));
        assert_eq!(
            tick.trade_id.to_string(),
            "9f2b2b0e-1c1a-4b0e-9f7a-2b6a5b1c9d10"
        );
    }

    #[rstest]
    fn test_order_book_deltas_invert_no_bids_into_yes_asks() {
        let orderbook: KalshiOrderbook = serde_json::from_str(
            r#"{
                "yes_dollars": [["0.3400", "120.00"]],
                "no_dollars": [["0.6500", "60.00"]]
            }"#,
        )
        .unwrap();

        let deltas =
            create_order_book_deltas_from_market(&market(), &orderbook, 7, UnixNanos::from(TS))
                .unwrap();

        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[0].action, BookAction::Clear);
        assert_eq!(deltas[1].order.side, Some(OrderSide::Buy));
        assert_eq!(deltas[1].order.price, Price::from("0.34"));
        assert_eq!(deltas[1].order.size, Quantity::from("120.00"));
        assert_eq!(deltas[2].order.side, Some(OrderSide::Sell));
        // A NO bid at 0.65 is a YES ask at 0.35.
        assert_eq!(deltas[2].order.price, Price::from("0.35"));
        assert_eq!(deltas[2].order.size, Quantity::from("60.00"));
        assert_eq!(deltas[0].sequence, 7);
        assert_eq!(deltas[2].sequence, 9);
    }
}
