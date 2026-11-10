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

//! Kalshi REST response decoding and pagination controls.

use std::collections::HashSet;

use nautilus_core::{
    UnixNanos, datetime::try_datetime_to_unix_nanos, params::Params, string::secret::SecretString,
};
use nautilus_model::{
    data::InstrumentClose,
    enums::{AssetClass, InstrumentCloseType},
    identifiers::Symbol,
    instruments::BinaryOption,
    types::{Currency, Price, Quantity},
};
use nautilus_network::http::HttpResponse;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use ustr::Ustr;

use super::{
    error::{KalshiHttpError, ResponseEvidence},
    models::{Event, Field, Market, VenueError},
};
use crate::common::{
    consts::{PRICE_PRECISION, SIZE_PRECISION},
    enums::{CollateralReturnType, MarketStatus, MarketType, SettlementBoundsType},
    parse::{KalshiPriceGrid, parse_instrument_id},
};

/// Decodes a successful envelope or retains typed HTTP and venue failure evidence.
///
/// # Errors
///
/// Returns an error if the status is not successful or the success body is malformed.
pub fn decode_response<T: DeserializeOwned>(response: HttpResponse) -> Result<T, KalshiHttpError> {
    let status = response.status.as_u16();
    if (200..300).contains(&status) {
        return decode_body(response);
    }

    let evidence = response_evidence(response);
    if evidence.venue.is_some() {
        Err(KalshiHttpError::Venue { status, evidence })
    } else {
        Err(KalshiHttpError::Http { status, evidence })
    }
}

pub(super) fn decode_body<T: DeserializeOwned>(
    response: HttpResponse,
) -> Result<T, KalshiHttpError> {
    serde_json::from_slice(&response.body).map_err(|e| KalshiHttpError::Decode {
        status: response.status.as_u16(),
        line: e.line(),
        column: e.column(),
        evidence: response_evidence(response),
    })
}

pub(super) fn unexpected_response(response: HttpResponse, reason: &'static str) -> KalshiHttpError {
    KalshiHttpError::Response {
        status: response.status.as_u16(),
        reason,
        evidence: response_evidence(response),
    }
}

fn response_evidence(response: HttpResponse) -> Box<ResponseEvidence> {
    let body = SecretString::from(String::from_utf8_lossy(&response.body).into_owned());
    let mut venue = serde_json::from_slice::<VenueError>(&response.body)
        .ok()
        .filter(|e| !e.code.is_missing() || !e.message.is_missing() || !e.details.is_missing());

    if venue.is_none() {
        #[derive(serde::Deserialize)]
        struct ErrorEnvelope {
            error: VenueError,
        }
        venue = serde_json::from_slice::<ErrorEnvelope>(&response.body)
            .ok()
            .map(|e| e.error);
    }

    let mut headers = response.headers;

    Box::new(ResponseEvidence {
        body,
        venue,
        retry_after: headers.remove("retry-after"),
        request_id: headers.remove("x-request-id"),
    })
}

/// A finite pagination result. A page budget never masquerades as exhaustion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pages<T> {
    /// Records collected within the page budget, deduplicated by source identity.
    pub records: Vec<T>,
    /// Whether the source was exhausted within the page budget.
    pub complete: bool,
    /// Continuation cursor for the next page, or `None` when the source is exhausted.
    pub next_cursor: Option<String>,
}

pub(crate) struct Pagination {
    cursors: HashSet<String>,
    identities: HashSet<String>,
}

impl Pagination {
    pub(crate) fn new(cursor: Option<&str>) -> Self {
        Self {
            cursors: cursor.into_iter().map(str::to_owned).collect(),
            identities: HashSet::new(),
        }
    }

    pub(super) fn retain(&mut self, identity: String) -> bool {
        self.identities.insert(identity)
    }

    pub(crate) fn advance(
        &mut self,
        cursor: Field<String>,
        empty: bool,
    ) -> Result<Option<String>, KalshiHttpError> {
        match cursor {
            Field::Missing | Field::Null => Ok(None),
            Field::Value(cursor) if cursor.is_empty() => Ok(None),
            Field::Value(cursor) => {
                if !self.cursors.insert(cursor.clone()) {
                    return Err(KalshiHttpError::Pagination("repeated cursor"));
                }

                if empty {
                    return Err(KalshiHttpError::Pagination(
                        "empty page with a continuation cursor",
                    ));
                }

                Ok(Some(cursor))
            }
        }
    }
}

/// Maps an admitted ordinary market to the canonical YES-priced instrument.
///
/// The retained market and event metadata restore grid and shard context on restart.
///
/// # Errors
///
/// Returns an error if economics are unsupported, identity or shard is inconsistent, or metadata is invalid.
pub fn parse_instrument(
    market: &Market,
    event: &Event,
    ts_init: UnixNanos,
) -> anyhow::Result<BinaryOption> {
    anyhow::ensure!(
        supported_market(market, event),
        "unsupported market economics or family"
    );
    anyhow::ensure!(
        market.event_ticker == event.event_ticker,
        "market and event identity mismatch"
    );
    parse_instrument_id(&event.event_ticker)?;
    parse_instrument_id(&event.series_ticker)?;

    let exchange_index = match market.exchange_index {
        Field::Value(v) if v >= 0 => v,
        _ => anyhow::bail!("missing or invalid market exchange index"),
    };

    anyhow::ensure!(
        matches!(event.exchange_index, Field::Missing | Field::Null)
            || event.exchange_index == Field::Value(exchange_index),
        "market and event shard mismatch"
    );
    let id = parse_instrument_id(&market.ticker)?;
    let grid = KalshiPriceGrid::new(&market.price_ranges)?;
    let tick_scheme = grid.register()?;
    let activation_ns = try_datetime_to_unix_nanos(market.open_time)?;
    let expiration_ns = try_datetime_to_unix_nanos(market.latest_expiration_time)?;
    anyhow::ensure!(
        activation_ns < expiration_ns,
        "invalid instrument lifecycle interval"
    );

    let ts_event = match market.updated_time {
        Field::Value(value) => try_datetime_to_unix_nanos(value)?,
        _ => ts_init,
    };

    let mut event_metadata = event.clone();
    event_metadata.markets = Field::Missing;

    let mut event_info = serde_json::to_value(&event_metadata)?;
    if let Field::Value(multiplier) = event_metadata.fee_multiplier_override {
        event_info["fee_multiplier_override"] = multiplier.0.to_string().into();
    }

    let info = Params::new(
        [
            ("market".into(), serde_json::to_value(market)?),
            ("event".into(), event_info),
        ]
        .into(),
    );

    let size_increment =
        Quantity::from_decimal_dp(Decimal::new(1, u32::from(SIZE_PRECISION)), SIZE_PRECISION)?;
    let instrument = BinaryOption::builder()
        .instrument_id(id)
        .raw_symbol(Symbol::new(&market.ticker))
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
        .outcome(Ustr::from("YES"))
        .description(Ustr::from(&market.yes_sub_title))
        .tick_scheme(tick_scheme)
        .info(info)
        .ts_event(ts_event)
        .ts_init(ts_init)
        .build()?;
    Ok(instrument)
}

pub(crate) fn supported_market(market: &Market, event: &Event) -> bool {
    market.market_type == MarketType::Binary
        && market.notional_value_dollars.0 == Decimal::ONE
        && market.settlement_bounds_type == SettlementBoundsType::Default
        && (event.collateral_return_type == CollateralReturnType::None
            || event.collateral_return_type == CollateralReturnType::Mecnet)
        && !matches!(&market.settlement_floor_dollars, Field::Value(v) if v.0 != Decimal::ZERO)
        && !matches!(&market.mve_collection_ticker, Field::Value(v) if !v.is_empty())
        && !matches!(&market.mve_selected_legs, Field::Value(v) if !v.is_empty())
}

/// Returns authoritative final YES payout evidence for an admitted ordinary binary market.
///
/// Returns `None` for non-finalized markets, including determined, disputed, and amended states.
/// This is market evidence, not a credited account amount. Account consumers must reconcile all
/// fills through the settlement timestamp before applying it and reconcile actual cash separately.
///
/// # Errors
///
/// Returns an error if a finalized definition has unsupported economics, invalid identity,
/// missing final evidence, an invalid settlement timestamp, or a payout outside `[0, 1]`
/// or beyond six decimal places.
pub fn parse_instrument_close(
    market: &Market,
    event: &Event,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<InstrumentClose>> {
    if market.status != MarketStatus::Finalized {
        return Ok(None);
    }

    let instrument = parse_instrument(market, event, ts_init)?;

    let Field::Value(value) = &market.settlement_value_dollars else {
        anyhow::bail!("finalized market has no settlement value");
    };

    let Field::Value(timestamp) = market.settlement_ts else {
        anyhow::bail!("finalized market has no settlement timestamp");
    };

    anyhow::ensure!(
        (Decimal::ZERO..=Decimal::ONE).contains(&value.0)
            && value.0.normalize().scale() <= u32::from(PRICE_PRECISION),
        "invalid final settlement value"
    );
    let price = Price::from_decimal_dp(value.0, PRICE_PRECISION)?;
    let ts_event = try_datetime_to_unix_nanos(timestamp)?;
    anyhow::ensure!(
        ts_event > UnixNanos::default(),
        "invalid settlement timestamp"
    );
    Ok(Some(InstrumentClose::new(
        instrument.id,
        price,
        InstrumentCloseType::ContractExpired,
        ts_event,
        ts_init,
    )))
}
