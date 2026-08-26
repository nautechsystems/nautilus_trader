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

//! Results from completed backtest runs.

use std::collections::{BTreeMap, BTreeSet};

use ahash::AHashMap;
use nautilus_analysis::PortfolioStatistics;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::{AccountAny, margin_model::MarginModel},
    events::{OrderEventAny, PortfolioSnapshot, PositionAdjusted},
    identifiers::InstrumentId,
    orders::{Order, OrderAny},
    position::{Position, PositionReplayEvent},
    types::{Currency, Money},
};
use serde::Serialize;
use serde_json::{Map, Value, json};

const CANONICAL_SCHEMA: &str = "nautilus-backtest-result/v1";
const METADATA_KEYS: &[&str] = &["exec_algorithm_params", "info"];
const UNORDERED_ARRAY_KEYS: &[&str] = &[
    "accounts",
    "actor_ids",
    "balances",
    "diagnostics",
    "exec_algorithm_ids",
    "fills",
    "linked_order_ids",
    "margins",
    "orders",
    "portfolio_snapshots",
    "position_snapshots",
    "positions",
    "realized_pnls",
    "stale_currencies",
    "stale_instruments",
    "strategy_ids",
    "tags",
    "total_equity",
    "trade_ids",
    "unpriced_instruments",
    "unrealized_pnls",
    "venue_order_ids",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IdentityClass {
    ClientOrder,
    Event,
    OrderList,
    Position,
    Trade,
    VenueOrder,
}

impl IdentityClass {
    const fn prefix(self) -> &'static str {
        match self {
            Self::ClientOrder => "client-order",
            Self::Event => "event",
            Self::OrderList => "order-list",
            Self::Position => "position",
            Self::Trade => "trade",
            Self::VenueOrder => "venue-order",
        }
    }
}

/// Results from a completed backtest run.
#[derive(Debug, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.backtest", skip_from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")
)]
pub struct BacktestResult {
    pub trader_id: String,
    pub machine_id: String,
    pub instance_id: UUID4,
    pub run_config_id: Option<String>,
    pub run_id: Option<UUID4>,
    pub run_started: Option<UnixNanos>,
    pub run_finished: Option<UnixNanos>,
    pub backtest_start: Option<UnixNanos>,
    pub backtest_end: Option<UnixNanos>,
    pub elapsed_time_secs: f64,
    pub iterations: usize,
    pub total_events: usize,
    pub total_orders: usize,
    pub total_positions: usize,
    pub summary: AHashMap<String, String>,
    pub stats_pnls: AHashMap<String, AHashMap<String, f64>>,
    pub stats_returns: AHashMap<String, f64>,
    pub stats_general: AHashMap<String, f64>,
    pub returns_series: BTreeMap<UnixNanos, f64>,
}

/// Versioned deterministic projection of observable backtest state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBacktestResult {
    document: Value,
}

/// The first difference between two canonical backtest results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CanonicalResultDivergence {
    /// RFC 6901 JSON Pointer to the differing field or record.
    pub path: String,
    /// The expected value, or `None` when the field or record is absent.
    pub expected: Option<Value>,
    /// The actual value, or `None` when the field or record is absent.
    pub actual: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CanonicalDiagnostic {
    pub code: CanonicalDiagnosticCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanonicalDiagnosticCode {
    FundingSettlementFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CanonicalRunOutcome {
    Completed,
    Failed,
    Incomplete,
    Stopped,
}

pub(crate) struct CanonicalBacktestState {
    pub trader_id: String,
    pub run_config_id: Option<String>,
    pub backtest_start: Option<UnixNanos>,
    pub backtest_end: Option<UnixNanos>,
    pub iterations: usize,
    pub total_events: usize,
    pub total_orders: usize,
    pub total_positions: usize,
    pub outcome: CanonicalRunOutcome,
    pub diagnostics: Vec<CanonicalDiagnostic>,
    pub trader_state: String,
    pub actor_ids: Vec<String>,
    pub strategy_ids: Vec<String>,
    pub exec_algorithm_ids: Vec<String>,
    pub summary: BTreeMap<String, String>,
    pub orders: Vec<OrderAny>,
    pub positions: Vec<Position>,
    pub position_snapshots: Vec<Position>,
    pub accounts: Vec<AccountAny>,
    pub portfolio_snapshots: Vec<PortfolioSnapshot>,
    pub statistics: PortfolioStatistics,
}

impl CanonicalBacktestResult {
    /// Decodes canonical result bytes and verifies their envelope, normalization, and exact
    /// encoding.
    ///
    /// Inner records retain their NautilusTrader model serialization and are compared as semantic
    /// content. Producers must use the version 1 writer rather than construct records independently.
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes are not a canonical version 1 result.
    pub fn from_slice(bytes: &[u8]) -> anyhow::Result<Self> {
        let document: Value = serde_json::from_slice(bytes)
            .map_err(|e| anyhow::anyhow!("invalid canonical backtest result JSON: {e}"))?;
        validate_document(&document)?;
        let mut normalized = document.clone();
        canonicalize_document(&mut normalized)?;
        anyhow::ensure!(
            normalized == document,
            "canonical backtest result violates the version 1 encoding rules"
        );
        let canonical = serde_json::to_vec(&normalized)?;
        anyhow::ensure!(
            canonical == bytes,
            "canonical backtest result bytes do not use the canonical encoding"
        );
        Ok(Self { document })
    }

    /// Returns the canonical compact UTF-8 JSON bytes without trailing data.
    ///
    /// # Errors
    ///
    /// Returns an error if the in-memory document cannot be serialized.
    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec(&self.document)?)
    }

    /// Returns `blake3:` followed by the 32-byte BLAKE3 digest as 64 lowercase hex digits.
    ///
    /// # Errors
    ///
    /// Returns an error if the in-memory document cannot be serialized.
    pub fn digest(&self) -> anyhow::Result<String> {
        let bytes = self.to_bytes()?;
        Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
    }

    /// Returns the decoded canonical document.
    #[must_use]
    pub const fn as_value(&self) -> &Value {
        &self.document
    }

    /// Returns the first field-level or record-level difference.
    #[must_use]
    pub fn first_divergence(&self, actual: &Self) -> Option<CanonicalResultDivergence> {
        first_divergence(&self.document, &actual.document, String::new())
    }

    pub(crate) fn from_state(mut state: CanonicalBacktestState) -> anyhow::Result<Self> {
        state.actor_ids.sort();
        state.strategy_ids.sort();
        state.exec_algorithm_ids.sort();

        let orders = state
            .orders
            .iter()
            .map(canonical_order)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let fills = canonical_fills(&state.orders)?;
        let positions = state
            .positions
            .iter()
            .map(canonical_position)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let position_snapshots = state
            .position_snapshots
            .iter()
            .map(canonical_position)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let accounts = state
            .accounts
            .iter()
            .map(canonical_account)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let portfolio_snapshots = state
            .portfolio_snapshots
            .iter()
            .map(canonical_value)
            .collect::<anyhow::Result<Vec<_>>>()?;

        let mut document = json!({
            "accounts": accounts,
            "components": {
                "actor_ids": state.actor_ids,
                "exec_algorithm_ids": state.exec_algorithm_ids,
                "strategy_ids": state.strategy_ids,
                "trader_state": state.trader_state,
            },
            "diagnostics": state.diagnostics,
            "fills": fills,
            "orders": orders,
            "portfolio_snapshots": portfolio_snapshots,
            "position_snapshots": position_snapshots,
            "positions": positions,
            "run": {
                "backtest_end_ns": optional_nanos(state.backtest_end),
                "backtest_start_ns": optional_nanos(state.backtest_start),
                "iterations": state.iterations.to_string(),
                "outcome": state.outcome,
                "run_config_id": state.run_config_id,
                "total_events": state.total_events.to_string(),
                "total_orders": state.total_orders.to_string(),
                "total_positions": state.total_positions.to_string(),
                "trader_id": state.trader_id,
            },
            "schema": CANONICAL_SCHEMA,
            "statistics": canonical_statistics(state.statistics),
            "summary": state.summary,
        });

        canonicalize_document(&mut document)?;
        validate_document(&document)?;
        Ok(Self { document })
    }
}

fn validate_document(document: &Value) -> anyhow::Result<()> {
    let object = document
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("canonical backtest result must be a JSON object"))?;
    anyhow::ensure!(
        object.get("schema").and_then(Value::as_str) == Some(CANONICAL_SCHEMA),
        "unsupported canonical backtest result schema"
    );
    let fields = [
        "accounts",
        "components",
        "diagnostics",
        "fills",
        "orders",
        "portfolio_snapshots",
        "position_snapshots",
        "positions",
        "run",
        "schema",
        "statistics",
        "summary",
    ];
    validate_fields(object, &fields, "canonical result")?;

    for key in [
        "accounts",
        "diagnostics",
        "fills",
        "orders",
        "portfolio_snapshots",
        "position_snapshots",
        "positions",
    ] {
        anyhow::ensure!(
            object.get(key).is_some_and(Value::is_array),
            "canonical result field '{key}' must be an array"
        );
    }
    validate_components(object.get("components").expect("validated field"))?;
    validate_diagnostics(object.get("diagnostics").expect("validated field"))?;
    validate_run(object.get("run").expect("validated field"))?;
    validate_statistics(object.get("statistics").expect("validated field"))?;
    let summary = object
        .get("summary")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("canonical result summary must be an object"))?;
    anyhow::ensure!(
        summary.values().all(Value::is_string),
        "canonical result summary values must be strings"
    );
    Ok(())
}

fn validate_fields(
    object: &Map<String, Value>,
    expected: &[&str],
    context: &str,
) -> anyhow::Result<()> {
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    anyhow::ensure!(
        actual == expected,
        "{context} fields do not match the version 1 schema"
    );
    Ok(())
}

fn validate_components(value: &Value) -> anyhow::Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("canonical result components must be an object"))?;
    validate_fields(
        object,
        &[
            "actor_ids",
            "exec_algorithm_ids",
            "strategy_ids",
            "trader_state",
        ],
        "canonical result components",
    )?;

    for key in ["actor_ids", "exec_algorithm_ids", "strategy_ids"] {
        let values = object
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("canonical component field '{key}' must be an array"))?;
        anyhow::ensure!(
            values.iter().all(Value::is_string),
            "canonical component field '{key}' must contain strings"
        );
    }
    anyhow::ensure!(
        object.get("trader_state").is_some_and(Value::is_string),
        "canonical trader state must be a string"
    );
    Ok(())
}

fn validate_diagnostics(value: &Value) -> anyhow::Result<()> {
    let diagnostics = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("canonical diagnostics must be an array"))?;
    for diagnostic in diagnostics {
        let object = diagnostic
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("canonical diagnostic must be an object"))?;
        validate_fields(object, &["code"], "canonical diagnostic")?;
        anyhow::ensure!(
            object.get("code").and_then(Value::as_str) == Some("funding-settlement-failed"),
            "unsupported canonical diagnostic code"
        );
    }
    Ok(())
}

fn validate_run(value: &Value) -> anyhow::Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("canonical result run must be an object"))?;
    validate_fields(
        object,
        &[
            "backtest_end_ns",
            "backtest_start_ns",
            "iterations",
            "outcome",
            "run_config_id",
            "total_events",
            "total_orders",
            "total_positions",
            "trader_id",
        ],
        "canonical result run",
    )?;

    for key in [
        "iterations",
        "total_events",
        "total_orders",
        "total_positions",
    ] {
        validate_unsigned_decimal(object.get(key).expect("validated field"), key, false)?;
    }

    for key in ["backtest_start_ns", "backtest_end_ns"] {
        validate_unsigned_decimal(object.get(key).expect("validated field"), key, true)?;
    }
    anyhow::ensure!(
        object
            .get("run_config_id")
            .is_some_and(|value| value.is_null() || value.is_string()),
        "canonical run configuration ID must be a string or null"
    );
    anyhow::ensure!(
        object.get("trader_id").is_some_and(Value::is_string),
        "canonical trader ID must be a string"
    );
    anyhow::ensure!(
        matches!(
            object.get("outcome").and_then(Value::as_str),
            Some("completed" | "failed" | "incomplete" | "stopped")
        ),
        "unsupported canonical run outcome"
    );
    Ok(())
}

fn validate_unsigned_decimal(value: &Value, field: &str, nullable: bool) -> anyhow::Result<()> {
    if nullable && value.is_null() {
        return Ok(());
    }
    let value = value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("canonical run field '{field}' must be a decimal string"))?;
    anyhow::ensure!(
        value == "0"
            || (!value.is_empty()
                && !value.starts_with('0')
                && value.bytes().all(|byte| byte.is_ascii_digit())),
        "canonical run field '{field}' is not a canonical unsigned decimal"
    );
    Ok(())
}

fn validate_statistics(value: &Value) -> anyhow::Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("canonical result statistics must be an object"))?;
    validate_fields(
        object,
        &["general", "pnls", "returns", "returns_series"],
        "canonical result statistics",
    )?;

    for key in ["general", "pnls", "returns"] {
        anyhow::ensure!(
            object.get(key).is_some_and(Value::is_object),
            "canonical statistics field '{key}' must be an object"
        );
    }
    anyhow::ensure!(
        object.get("returns_series").is_some_and(Value::is_array),
        "canonical returns series must be an array"
    );
    Ok(())
}

fn optional_nanos(value: Option<UnixNanos>) -> Value {
    value.map_or(Value::Null, |nanos| Value::String(nanos.to_string()))
}

fn canonical_order(order: &OrderAny) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(order)?;
    let payload = variant_payload_mut(&mut value)?;
    let core = payload
        .get_mut("core")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("serialized order did not contain an object core"))?;
    set_decimal(core, "avg_px", order.avg_px());
    set_decimal(core, "slippage", order.slippage());

    if let Some(events) = core.get_mut("events").and_then(Value::as_array_mut) {
        for (source, encoded) in order.events().into_iter().zip(events) {
            patch_order_event_decimals(source, encoded)?;
        }
    }
    set_decimal_if_present(payload, "limit_offset", order.limit_offset());
    set_decimal_if_present(payload, "trailing_offset", order.trailing_offset());
    canonicalize_value(&mut value)?;
    Ok(value)
}

fn canonical_fills(orders: &[OrderAny]) -> anyhow::Result<Vec<Value>> {
    let mut fills = Vec::new();

    for order in orders {
        for (ordinal, event) in order.events().into_iter().enumerate() {
            if !matches!(event, OrderEventAny::Filled(_)) {
                continue;
            }
            let mut encoded = serde_json::to_value(event)?;
            canonicalize_value(&mut encoded)?;
            fills.push(json!({
                "client_order_id": order.client_order_id().to_string(),
                "event": encoded,
                "order_event_ordinal": ordinal.to_string(),
            }));
        }
    }
    Ok(fills)
}

fn canonical_position(position: &Position) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(position)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("serialized position was not an object"))?;
    anyhow::ensure!(
        object.remove("id").is_some(),
        "serialized position did not contain its identifier"
    );
    object.insert(
        "position_id".to_string(),
        Value::String(position.id.to_string()),
    );
    set_f64(object, "avg_px_close", position.avg_px_close);
    set_f64(object, "avg_px_open", Some(position.avg_px_open));
    set_f64(object, "realized_return", Some(position.realized_return));
    set_f64(object, "signed_qty", Some(position.signed_qty));

    if let Some(adjustments) = object.get_mut("adjustments").and_then(Value::as_array_mut) {
        for (source, encoded) in position.adjustments.iter().zip(adjustments) {
            patch_position_adjustment(source, encoded)?;
        }
    }

    if let Some(events) = object
        .get_mut("replay_events")
        .and_then(Value::as_array_mut)
    {
        for (source, encoded) in position.replay_events.iter().zip(events) {
            if let PositionReplayEvent::Adjusted(adjustment) = source {
                set_decimal(
                    variant_payload_mut(encoded)?,
                    "quantity_change",
                    adjustment.quantity_change,
                );
            }
        }
    }
    canonicalize_value(&mut value)?;
    Ok(value)
}

fn canonical_account(account: &AccountAny) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(account)?;
    let payload = variant_payload_mut(&mut value)?;

    match account {
        AccountAny::Margin(margin) => {
            let leverages = margin
                .leverages
                .iter()
                .map(|(instrument_id, leverage)| {
                    (
                        instrument_id.to_string(),
                        Value::String(canonical_decimal(*leverage)),
                    )
                })
                .collect::<Map<_, _>>();
            let margin_model = margin.margin_model().name();
            payload.insert(
                "default_leverage".to_string(),
                Value::String(canonical_decimal(margin.default_leverage)),
            );
            payload.insert("leverages".to_string(), Value::Object(leverages));
            payload.insert(
                "margin_model".to_string(),
                Value::String(margin_model.to_string()),
            );
        }
        AccountAny::Cash(cash) => {
            payload.insert(
                "balances_locked_transient".to_string(),
                locked_balances(&cash.balances_locked),
            );
        }
        AccountAny::Betting(betting) => {
            payload.insert(
                "balances_locked_transient".to_string(),
                locked_balances(&betting.balances_locked),
            );
        }
        AccountAny::Wallet(wallet) => {
            payload.insert(
                "balances_locked_transient".to_string(),
                locked_balances(&wallet.balances_locked),
            );
        }
    }
    canonicalize_value(&mut value)?;
    Ok(value)
}

fn locked_balances(balances: &AHashMap<(InstrumentId, Currency), Money>) -> Value {
    let mut values = balances
        .iter()
        .map(|((instrument_id, currency), money)| {
            json!({
                "currency": currency.code.to_string(),
                "instrument_id": instrument_id.to_string(),
                "money": money.to_string(),
            })
        })
        .collect::<Vec<_>>();
    values.sort_by_cached_key(|value| canonical_sort_key(value, true));
    Value::Array(values)
}

fn canonical_statistics(statistics: PortfolioStatistics) -> Value {
    let pnls = statistics
        .pnls
        .into_iter()
        .map(|(currency, values)| (currency, canonical_f64_map(values)))
        .collect::<Map<_, _>>();
    let returns_series = statistics
        .returns_series
        .into_iter()
        .map(|(timestamp, value)| {
            json!({
                "timestamp_ns": timestamp.to_string(),
                "value": canonical_f64(value),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "general": canonical_f64_map(statistics.general),
        "pnls": pnls,
        "returns": canonical_f64_map(statistics.returns),
        "returns_series": returns_series,
    })
}

fn canonical_f64_map(values: AHashMap<String, f64>) -> Value {
    Value::Object(
        values
            .into_iter()
            .map(|(name, value)| (name, Value::String(canonical_f64(value))))
            .collect(),
    )
}

fn canonical_f64(value: f64) -> String {
    if value.is_nan() {
        "nan".to_string()
    } else if value == f64::INFINITY {
        "+inf".to_string()
    } else if value == f64::NEG_INFINITY {
        "-inf".to_string()
    } else {
        format!("{:016x}", value.to_bits())
    }
}

fn canonical_decimal(value: rust_decimal::Decimal) -> String {
    value.normalize().to_string()
}

fn set_f64(object: &mut Map<String, Value>, key: &str, value: Option<f64>) {
    object.insert(
        key.to_string(),
        value.map_or(Value::Null, |value| Value::String(canonical_f64(value))),
    );
}

fn set_decimal(object: &mut Map<String, Value>, key: &str, value: Option<rust_decimal::Decimal>) {
    object.insert(
        key.to_string(),
        value.map_or(Value::Null, |value| Value::String(canonical_decimal(value))),
    );
}

fn set_decimal_if_present(
    object: &mut Map<String, Value>,
    key: &str,
    value: Option<rust_decimal::Decimal>,
) {
    if object.contains_key(key) {
        set_decimal(object, key, value);
    }
}

fn patch_order_event_decimals(event: &OrderEventAny, value: &mut Value) -> anyhow::Result<()> {
    if let OrderEventAny::Initialized(initialized) = event {
        let payload = variant_payload_mut(value)?;
        set_decimal(payload, "limit_offset", initialized.limit_offset);
        set_decimal(payload, "trailing_offset", initialized.trailing_offset);
    }
    Ok(())
}

fn patch_position_adjustment(
    adjustment: &PositionAdjusted,
    value: &mut Value,
) -> anyhow::Result<()> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("serialized position adjustment was not an object"))?;
    set_decimal(object, "quantity_change", adjustment.quantity_change);
    Ok(())
}

fn variant_payload_mut(value: &mut Value) -> anyhow::Result<&mut Map<String, Value>> {
    let variant = value
        .as_object_mut()
        .and_then(|object| object.values_mut().next())
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("serialized enum variant was not an object"))?;
    Ok(variant)
}

fn canonical_value<T: Serialize>(source: &T) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(source)?;
    canonicalize_value(&mut value)?;
    Ok(value)
}

fn canonicalize_value(value: &mut Value) -> anyhow::Result<()> {
    value.sort_all_objects();
    sort_named_arrays(value, false);
    stringify_numbers(value)?;
    Ok(())
}

fn canonicalize_document(value: &mut Value) -> anyhow::Result<()> {
    value.sort_all_objects();
    sort_named_arrays(value, false);
    normalize_identities(value);
    sort_named_arrays(value, true);
    stringify_numbers(value)
}

fn sort_named_arrays(value: &mut Value, identities_normalized: bool) {
    match value {
        Value::Array(values) => {
            for value in values {
                sort_named_arrays(value, identities_normalized);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                sort_named_arrays(value, identities_normalized);
                if UNORDERED_ARRAY_KEYS.contains(&key.as_str())
                    && let Value::Array(values) = value
                    && (identities_normalized || identity_array_class(key).is_none())
                {
                    values.sort_by_cached_key(|value| {
                        canonical_sort_key(value, !identities_normalized)
                    });
                }
            }
        }
        _ => {}
    }
}

fn canonical_sort_key(value: &Value, strip_identity_fields: bool) -> Vec<u8> {
    let mut value = value.clone();
    if strip_identity_fields {
        strip_identities(&mut value, false);
    }
    serde_json::to_vec(&value).expect("serializing a JSON value cannot fail")
}

fn strip_identities(value: &mut Value, metadata: bool) {
    match value {
        Value::Array(values) => {
            for value in values {
                strip_identities(value, metadata);
            }
        }
        Value::Object(object) => {
            if !metadata {
                object.retain(|key, _| {
                    identity_scalar_class(key).is_none() && identity_array_class(key).is_none()
                });
            }

            for (key, value) in object {
                strip_identities(value, metadata || METADATA_KEYS.contains(&key.as_str()));
            }
        }
        _ => {}
    }
}

fn normalize_identities(document: &mut Value) {
    let mut identities = BTreeMap::<IdentityClass, BTreeMap<String, String>>::new();
    collect_primary_identities(document, false, &mut identities);
    let mut next_external = BTreeMap::<IdentityClass, usize>::new();
    replace_identities(document, false, &mut identities, &mut next_external);
}

fn collect_primary_identities(
    value: &Value,
    metadata: bool,
    identities: &mut BTreeMap<IdentityClass, BTreeMap<String, String>>,
) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_primary_identities(value, metadata, identities);
            }
        }
        Value::Object(object) => {
            if !metadata {
                for (key, value) in object {
                    let Some(class) = primary_identity_class(key) else {
                        continue;
                    };
                    let Some(identity) = value.as_str() else {
                        continue;
                    };
                    let class_identities = identities.entry(class).or_default();
                    let next = class_identities.len() + 1;
                    class_identities
                        .entry(identity.to_string())
                        .or_insert_with(|| format!("{}-{next}", class.prefix()));
                }
            }

            for (key, value) in object {
                collect_primary_identities(
                    value,
                    metadata || METADATA_KEYS.contains(&key.as_str()),
                    identities,
                );
            }
        }
        _ => {}
    }
}

fn replace_identities(
    value: &mut Value,
    metadata: bool,
    identities: &mut BTreeMap<IdentityClass, BTreeMap<String, String>>,
    next_external: &mut BTreeMap<IdentityClass, usize>,
) {
    match value {
        Value::Array(values) => {
            for value in values {
                replace_identities(value, metadata, identities, next_external);
            }
        }
        Value::Object(object) => {
            if !metadata {
                for (key, value) in object.iter_mut() {
                    if let Some(class) = identity_scalar_class(key) {
                        replace_identity(value, class, identities, next_external);
                    } else if let Some(class) = identity_array_class(key)
                        && let Value::Array(values) = value
                    {
                        for value in values {
                            replace_identity(value, class, identities, next_external);
                        }
                    }
                }
            }

            for (key, value) in object {
                replace_identities(
                    value,
                    metadata || METADATA_KEYS.contains(&key.as_str()),
                    identities,
                    next_external,
                );
            }
        }
        _ => {}
    }
}

fn replace_identity(
    value: &mut Value,
    class: IdentityClass,
    identities: &mut BTreeMap<IdentityClass, BTreeMap<String, String>>,
    next_external: &mut BTreeMap<IdentityClass, usize>,
) {
    let Some(raw) = value.as_str() else {
        return;
    };
    let class_identities = identities.entry(class).or_default();
    let token = class_identities.entry(raw.to_string()).or_insert_with(|| {
        let next = next_external.entry(class).or_default();
        *next += 1;
        format!("{}-external-{next}", class.prefix())
    });
    *value = Value::String(token.clone());
}

fn primary_identity_class(key: &str) -> Option<IdentityClass> {
    match key {
        "client_order_id" => Some(IdentityClass::ClientOrder),
        "event_id" => Some(IdentityClass::Event),
        "order_list_id" => Some(IdentityClass::OrderList),
        "position_id" => Some(IdentityClass::Position),
        "trade_id" => Some(IdentityClass::Trade),
        "venue_order_id" => Some(IdentityClass::VenueOrder),
        _ => None,
    }
}

fn identity_scalar_class(key: &str) -> Option<IdentityClass> {
    match key {
        "client_order_id" | "closing_order_id" | "exec_spawn_id" | "opening_order_id"
        | "parent_order_id" => Some(IdentityClass::ClientOrder),
        "causation_id" | "event_id" | "init_id" => Some(IdentityClass::Event),
        "order_list_id" => Some(IdentityClass::OrderList),
        "position_id" => Some(IdentityClass::Position),
        "last_trade_id" | "trade_id" => Some(IdentityClass::Trade),
        "venue_order_id" => Some(IdentityClass::VenueOrder),
        _ => None,
    }
}

fn identity_array_class(key: &str) -> Option<IdentityClass> {
    match key {
        "linked_order_ids" => Some(IdentityClass::ClientOrder),
        "trade_ids" => Some(IdentityClass::Trade),
        "venue_order_ids" => Some(IdentityClass::VenueOrder),
        _ => None,
    }
}

fn stringify_numbers(value: &mut Value) -> anyhow::Result<()> {
    match value {
        Value::Array(values) => {
            for value in values {
                stringify_numbers(value)?;
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                stringify_numbers(value)?;
            }
        }
        Value::Number(number) => {
            anyhow::ensure!(
                number.is_i64() || number.is_u64(),
                "canonical projection contains an unencoded floating-point value"
            );
            *value = Value::String(number.to_string());
        }
        _ => {}
    }
    Ok(())
}

fn first_divergence(
    expected: &Value,
    actual: &Value,
    path: String,
) -> Option<CanonicalResultDivergence> {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let keys = expected
                .keys()
                .chain(actual.keys())
                .cloned()
                .collect::<BTreeSet<_>>();

            for key in keys {
                let next_path = format!("{path}/{}", escape_pointer_token(&key));
                match (expected.get(&key), actual.get(&key)) {
                    (Some(expected), Some(actual)) => {
                        if let Some(divergence) = first_divergence(expected, actual, next_path) {
                            return Some(divergence);
                        }
                    }
                    (expected, actual) => {
                        return Some(CanonicalResultDivergence {
                            path: next_path,
                            expected: expected.cloned(),
                            actual: actual.cloned(),
                        });
                    }
                }
            }
            None
        }
        (Value::Array(expected), Value::Array(actual)) => {
            let len = expected.len().max(actual.len());
            for index in 0..len {
                let next_path = format!("{path}/{index}");
                match (expected.get(index), actual.get(index)) {
                    (Some(expected), Some(actual)) => {
                        if let Some(divergence) = first_divergence(expected, actual, next_path) {
                            return Some(divergence);
                        }
                    }
                    (expected, actual) => {
                        return Some(CanonicalResultDivergence {
                            path: next_path,
                            expected: expected.cloned(),
                            actual: actual.cloned(),
                        });
                    }
                }
            }
            None
        }
        _ if expected == actual => None,
        _ => Some(CanonicalResultDivergence {
            path,
            expected: Some(expected.clone()),
            actual: Some(actual.clone()),
        }),
    }
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use nautilus_model::{
        accounts::{
            MarginAccount, WalletAccount,
            margin_model::{LeveragedMarginModel, MarginModelAny, StandardMarginModel},
        },
        enums::{AccountType, OrderType, TrailingOffsetType},
        events::AccountState,
        identifiers::{AccountId, InstrumentId},
        orders::OrderTestBuilder,
        types::{AccountBalance, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_backtest_result_serializes_to_json() {
        let instance_id = UUID4::from("11111111-1111-4111-8111-111111111111");
        let run_id = UUID4::from("22222222-2222-4222-8222-222222222222");
        let mut summary = AHashMap::new();
        summary.insert("PnL (total)".to_string(), "10.00 USD".to_string());
        let mut usd_pnls = AHashMap::new();
        usd_pnls.insert("Returns Volatility (252 days)".to_string(), 1.25);
        let mut stats_pnls = AHashMap::new();
        stats_pnls.insert("USD".to_string(), usd_pnls);
        let mut stats_returns = AHashMap::new();
        stats_returns.insert("Sharpe Ratio (252 days)".to_string(), 0.75);
        let mut stats_general = AHashMap::new();
        stats_general.insert("Long Ratio".to_string(), 1.0);

        let result = BacktestResult {
            trader_id: "TRADER-001".to_string(),
            machine_id: "machine-1".to_string(),
            instance_id,
            run_config_id: Some("config-1".to_string()),
            run_id: Some(run_id),
            run_started: Some(UnixNanos::new(1)),
            run_finished: Some(UnixNanos::new(2)),
            backtest_start: Some(UnixNanos::new(3)),
            backtest_end: Some(UnixNanos::new(4)),
            elapsed_time_secs: 1.5,
            iterations: 10,
            total_events: 20,
            total_orders: 2,
            total_positions: 1,
            summary,
            stats_pnls,
            stats_returns,
            stats_general,
            returns_series: BTreeMap::from([(UnixNanos::new(3), 0.25)]),
        };

        let value = serde_json::to_value(&result).unwrap();

        assert_eq!(value["trader_id"], json!("TRADER-001"));
        assert_eq!(value["machine_id"], json!("machine-1"));
        assert_eq!(value["instance_id"], json!(instance_id.to_string()));
        assert_eq!(value["run_id"], json!(run_id.to_string()));
        assert_eq!(value["run_started"], json!(1));
        assert_eq!(value["backtest_end"], json!(4));
        assert_eq!(value["elapsed_time_secs"], json!(1.5));
        assert_eq!(value["iterations"], json!(10));
        assert_eq!(value["summary"]["PnL (total)"], json!("10.00 USD"));
        assert_eq!(
            value["stats_pnls"]["USD"]["Returns Volatility (252 days)"],
            json!(1.25)
        );
        assert_eq!(
            value["stats_returns"]["Sharpe Ratio (252 days)"],
            json!(0.75)
        );
        assert_eq!(value["stats_general"]["Long Ratio"], json!(1.0));
        assert_eq!(value["returns_series"]["3"], json!(0.25));
    }

    #[rstest]
    fn test_canonical_account_wallet_includes_transient_locks() {
        let eth = Currency::ETH();
        let state = AccountState::new(
            AccountId::from("WALLET-001"),
            AccountType::Wallet,
            vec![AccountBalance::new(
                Money::new(10.0, eth),
                Money::zero(eth),
                Money::new(10.0, eth),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        let mut account = WalletAccount::new(state, true);
        let instrument_id = InstrumentId::from("WETHUSDC.BLOCKCHAIN");
        account
            .update_balance_locked(instrument_id, Money::new(2.0, eth))
            .unwrap();

        let value = canonical_account(&AccountAny::Wallet(account)).unwrap();

        let payload = value.get("Wallet").unwrap();
        let locks = payload["balances_locked_transient"].as_array().unwrap();
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0]["currency"], "ETH");
        assert_eq!(locks[0]["instrument_id"], "WETHUSDC.BLOCKCHAIN");
        assert_eq!(locks[0]["money"], "2.00000000 ETH");
    }

    #[rstest]
    fn test_canonical_margin_account_preserves_builtin_model_names() {
        let state = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1_000_000 USD"),
                Money::zero(Currency::USD()),
                Money::from("1_000_000 USD"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        let mut account = MarginAccount::new(state, true);

        for (model, expected_name) in [
            (MarginModelAny::Standard(StandardMarginModel), "standard"),
            (MarginModelAny::Leveraged(LeveragedMarginModel), "leveraged"),
        ] {
            account.set_margin_model(model.into());
            let value = canonical_account(&AccountAny::Margin(account.clone())).unwrap();

            assert_eq!(value["Margin"]["margin_model"], expected_name);
        }
    }

    #[rstest]
    fn test_canonical_f64_encodes_finite_and_non_finite_values() {
        let nan_with_payload = f64::from_bits(0x7ff8_0000_0000_0042);

        assert_eq!(canonical_f64(1.5), "3ff8000000000000");
        assert_eq!(canonical_f64(-0.0), "8000000000000000");
        assert_eq!(canonical_f64(f64::NAN), "nan");
        assert_eq!(canonical_f64(nan_with_payload), "nan");
        assert_eq!(canonical_f64(f64::INFINITY), "+inf");
        assert_eq!(canonical_f64(f64::NEG_INFINITY), "-inf");
    }

    #[rstest]
    fn test_canonical_decimal_removes_redundant_scale() {
        let value = "001.2300".parse().unwrap();

        assert_eq!(canonical_decimal(value), "1.23");
    }

    #[rstest]
    fn test_canonical_order_normalizes_core_and_event_decimals() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id(InstrumentId::from("AUDUSD.SIM"))
            .quantity(Quantity::from(100_000))
            .limit_offset(Decimal::new(12_300, 4))
            .trailing_offset(Decimal::new(45_600, 4))
            .trailing_offset_type(TrailingOffsetType::Price)
            .build();

        let value = canonical_order(&order).unwrap();
        let payload = value["TrailingStopLimit"].as_object().unwrap();
        let core = payload["core"].as_object().unwrap();
        let initialized = &core["events"][0]["Initialized"];

        assert!(!payload.contains_key("avg_px"));
        assert!(!payload.contains_key("slippage"));
        assert_eq!(payload["limit_offset"], "1.23");
        assert_eq!(payload["trailing_offset"], "4.56");
        assert_eq!(core["avg_px"], Value::Null);
        assert_eq!(core["slippage"], Value::Null);
        assert_eq!(initialized["limit_offset"], "1.23");
        assert_eq!(initialized["trailing_offset"], "4.56");
    }

    #[rstest]
    fn test_normalize_identities_preserves_event_relationships() {
        let mut first = json!({
            "events": [
                {"event_id": "11111111-1111-4111-8111-111111111111"},
                {
                    "causation_id": "11111111-1111-4111-8111-111111111111",
                    "event_id": "22222222-2222-4222-8222-222222222222",
                    "init_id": "22222222-2222-4222-8222-222222222222"
                }
            ]
        });
        let mut repeated = json!({
            "events": [
                {"event_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"},
                {
                    "causation_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                    "event_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                    "init_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
                }
            ]
        });

        normalize_identities(&mut first);
        normalize_identities(&mut repeated);

        assert_eq!(first, repeated);
        assert_eq!(first["events"][0]["event_id"], "event-1");
        assert_eq!(first["events"][1]["event_id"], "event-2");
        assert_eq!(first["events"][1]["init_id"], "event-2");
        assert_eq!(first["events"][1]["causation_id"], "event-1");
    }

    #[rstest]
    fn test_canonicalize_document_normalizes_random_domain_identities() {
        let mut first = test_document();
        first["fills"] = json!([
            {
                "client_order_id": "11111111-1111-4111-8111-111111111111",
                "event": {
                    "Filled": {
                        "event_id": "22222222-2222-4222-8222-222222222222",
                        "position_id": "33333333-3333-4333-8333-333333333333",
                        "trade_id": "44444444-4444-4444-8444-444444444444",
                        "venue_order_id": "55555555-5555-4555-8555-555555555555"
                    }
                },
                "order_event_ordinal": "1"
            }
        ]);
        first["orders"] = json!([
            {
                "Market": {
                    "core": {
                        "client_order_id": "11111111-1111-4111-8111-111111111111",
                        "position_id": "33333333-3333-4333-8333-333333333333",
                        "trade_ids": ["44444444-4444-4444-8444-444444444444"],
                        "venue_order_id": "55555555-5555-4555-8555-555555555555"
                    }
                }
            }
        ]);
        first["positions"] = json!([
            {
                "position_id": "33333333-3333-4333-8333-333333333333"
            }
        ]);
        let mut repeated = first.clone();
        replace_test_identity(&mut repeated["fills"]);
        replace_test_identity(&mut repeated["orders"]);
        replace_test_identity(&mut repeated["positions"]);

        canonicalize_document(&mut first).unwrap();
        canonicalize_document(&mut repeated).unwrap();

        assert_eq!(first, repeated);
        assert_eq!(first["fills"][0]["client_order_id"], "client-order-1");
        assert_eq!(first["fills"][0]["event"]["Filled"]["event_id"], "event-1");
        assert_eq!(
            first["fills"][0]["event"]["Filled"]["position_id"],
            "position-1"
        );
        assert_eq!(first["fills"][0]["event"]["Filled"]["trade_id"], "trade-1");
        assert_eq!(
            first["fills"][0]["event"]["Filled"]["venue_order_id"],
            "venue-order-1"
        );
        assert_eq!(first["positions"][0]["position_id"], "position-1");
    }

    #[rstest]
    fn test_canonicalize_document_orders_records_by_normalized_identity() {
        let mut first = test_document();
        first["fills"] = json!([
            {
                "event": {
                    "Filled": {
                        "position_id": "11111111-1111-4111-8111-111111111111"
                    }
                }
            },
            {
                "event": {
                    "Filled": {
                        "position_id": "22222222-2222-4222-8222-222222222222"
                    }
                }
            }
        ]);
        first["positions"] = json!([
            {
                "position_id": "11111111-1111-4111-8111-111111111111",
                "side": "LONG"
            },
            {
                "position_id": "22222222-2222-4222-8222-222222222222",
                "side": "LONG"
            }
        ]);
        let mut reordered = first.clone();
        reordered["positions"].as_array_mut().unwrap().reverse();

        canonicalize_document(&mut first).unwrap();
        canonicalize_document(&mut reordered).unwrap();

        assert_eq!(first, reordered);
        assert_eq!(reordered["positions"][0]["position_id"], "position-1");
        assert_eq!(reordered["positions"][1]["position_id"], "position-2");
    }

    #[rstest]
    fn test_canonical_result_reports_first_divergence_with_escaped_pointer() {
        let mut expected_document = test_document();
        expected_document["summary"] = json!({"order/price~open": "1.00000"});
        let mut actual_document = expected_document.clone();
        actual_document["summary"]["order/price~open"] = json!("1.00001");
        let expected = CanonicalBacktestResult {
            document: expected_document,
        };
        let actual = CanonicalBacktestResult {
            document: actual_document,
        };

        let divergence = expected.first_divergence(&actual).unwrap();

        assert_eq!(divergence.path, "/summary/order~1price~0open");
        assert_eq!(divergence.expected, Some(json!("1.00000")));
        assert_eq!(divergence.actual, Some(json!("1.00001")));
    }

    #[rstest]
    fn test_canonical_result_reports_missing_record() {
        let mut expected_document = test_document();
        expected_document["fills"] = json!([{"trade_id": "T-001"}]);
        let expected = CanonicalBacktestResult {
            document: expected_document,
        };
        let actual = CanonicalBacktestResult {
            document: test_document(),
        };

        let divergence = expected.first_divergence(&actual).unwrap();

        assert_eq!(divergence.path, "/fills/0");
        assert_eq!(divergence.expected, Some(json!({"trade_id": "T-001"})));
        assert_eq!(divergence.actual, None);
    }

    #[rstest]
    fn test_canonical_result_rejects_non_canonical_bytes() {
        let document = test_document();
        let canonical = serde_json::to_vec(&document).unwrap();
        let pretty = serde_json::to_vec_pretty(&document).unwrap();

        let result = CanonicalBacktestResult::from_slice(&canonical).unwrap();
        let error = CanonicalBacktestResult::from_slice(&pretty).unwrap_err();

        assert_eq!(result.as_value(), &document);
        assert_eq!(
            error.to_string(),
            "canonical backtest result bytes do not use the canonical encoding"
        );
    }

    #[rstest]
    fn test_canonical_result_rejects_wrong_schema() {
        let mut document = test_document();
        document["schema"] = json!("nautilus-backtest-result/v2");

        let error = CanonicalBacktestResult::from_slice(&serde_json::to_vec(&document).unwrap())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "unsupported canonical backtest result schema"
        );
    }

    #[rstest]
    fn test_canonical_result_rejects_non_v1_fields() {
        let mut document = test_document();
        document["unexpected"] = Value::Null;

        let error = CanonicalBacktestResult::from_slice(&serde_json::to_vec(&document).unwrap())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "canonical result fields do not match the version 1 schema"
        );
    }

    #[rstest]
    fn test_canonical_result_rejects_non_v1_numeric_encoding() {
        let mut document = test_document();
        document["run"]["iterations"] = json!(1);

        let error = CanonicalBacktestResult::from_slice(&serde_json::to_vec(&document).unwrap())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "canonical run field 'iterations' must be a decimal string"
        );
    }

    #[rstest]
    fn test_canonical_result_rejects_non_v1_collection_order() {
        let mut document = test_document();
        document["components"]["actor_ids"] = json!(["ACTOR-002", "ACTOR-001"]);

        let error = CanonicalBacktestResult::from_slice(&serde_json::to_vec(&document).unwrap())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "canonical backtest result violates the version 1 encoding rules"
        );
    }

    #[rstest]
    fn test_canonical_result_digest_covers_exact_bytes() {
        let first = CanonicalBacktestResult {
            document: test_document(),
        };
        let mut changed_document = test_document();
        changed_document["run"]["iterations"] = json!("2");
        let changed = CanonicalBacktestResult {
            document: changed_document,
        };

        let digest = first.digest().unwrap();
        let changed_digest = changed.digest().unwrap();

        assert_eq!(digest.len(), "blake3:".len() + 64);
        assert!(digest.starts_with("blake3:"));
        assert_ne!(digest, changed_digest);
    }

    #[rstest]
    fn test_stringify_numbers_rejects_unencoded_float() {
        let mut value = json!({"value": 1.25});

        let error = stringify_numbers(&mut value).unwrap_err();

        assert_eq!(
            error.to_string(),
            "canonical projection contains an unencoded floating-point value"
        );
    }

    fn test_document() -> Value {
        json!({
            "accounts": [],
            "components": {
                "actor_ids": [],
                "exec_algorithm_ids": [],
                "strategy_ids": [],
                "trader_state": "STOPPED",
            },
            "diagnostics": [],
            "fills": [],
            "orders": [],
            "portfolio_snapshots": [],
            "position_snapshots": [],
            "positions": [],
            "run": {
                "backtest_end_ns": "2",
                "backtest_start_ns": "1",
                "iterations": "1",
                "outcome": "completed",
                "run_config_id": null,
                "total_events": "0",
                "total_orders": "0",
                "total_positions": "0",
                "trader_id": "TRADER-001",
            },
            "schema": CANONICAL_SCHEMA,
            "statistics": {
                "general": {},
                "pnls": {},
                "returns": {},
                "returns_series": [],
            },
            "summary": {},
        })
    }

    fn replace_test_identity(value: &mut Value) {
        match value {
            Value::Array(values) => {
                for value in values {
                    replace_test_identity(value);
                }
            }
            Value::Object(object) => {
                for value in object.values_mut() {
                    replace_test_identity(value);
                }
            }
            Value::String(value) if value.contains('-') && value.len() == 36 => {
                *value = value
                    .replace('1', "a")
                    .replace('2', "b")
                    .replace('3', "c")
                    .replace('4', "d")
                    .replace('5', "e");
            }
            _ => {}
        }
    }
}
