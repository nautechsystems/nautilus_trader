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

use std::{cmp, str::FromStr};

use anyhow::Context;
use nautilus_common::messages::data::{DataResponse, RequestBars, RequestCommand, SubscribeBars};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::BarType,
    enums::{AggregationSource, ContinuousFutureAdjustmentType, PriceType},
    identifiers::{ClientId, InstrumentId},
};
use rust_decimal::Decimal;
use serde_json::Value;

pub(super) const CONTINUOUS_FUTURE_PARENT_REQUEST_ID: &str = "continuous_future_parent_request_id";

const CONTINUOUS_FUTURE_TRANSITIONS: &str = "continuous_future_transitions";
const CONTINUOUS_FUTURE_ADJUSTMENT_MODE: &str = "continuous_future_adjustment_mode";
const LAST_POST_INSTRUMENT_ID: &str = "last_post_instrument_id";
const FIRST_PRE_INSTRUMENT_ID: &str = "first_pre_instrument_id";
const BAR_TYPES: &str = "bar_types";

#[derive(Debug, Clone)]
pub(super) struct RequestBarAggregation {
    pub(super) bar_types: Vec<BarType>,
    pub(super) update_subscriptions: bool,
}

impl RequestBarAggregation {
    pub(super) const fn aggregator_request_id(&self, request_id: UUID4) -> Option<UUID4> {
        if self.update_subscriptions {
            None
        } else {
            Some(request_id)
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ContinuousFutureRequest {
    pub(super) primary_bar_type: BarType,
    pub(super) request_bar_aggregation: RequestBarAggregation,
    pub(super) transitions: Vec<ContinuousFutureTransition>,
    pub(super) adjustment_mode: ContinuousFutureAdjustmentType,
    first_pre_instrument_id: Option<InstrumentId>,
    last_post_instrument_id: Option<InstrumentId>,
}

#[derive(Debug, Clone)]
pub(super) struct ContinuousFutureRequestState {
    pub(super) parent: RequestBars,
    pub(super) request: ContinuousFutureRequest,
    pub(super) start_ns: UnixNanos,
    pub(super) cursor_ns: UnixNanos,
    pub(super) end_ns: UnixNanos,
    pub(super) response_client_id: ClientId,
    pub(super) data_count: u64,
}

impl ContinuousFutureRequest {
    pub(super) fn first_segment_instrument_id(&self) -> InstrumentId {
        self.transitions[0].pre_instrument_id
    }

    pub(super) fn next_segment(
        &self,
        cursor_ns: u64,
        end_ns: u64,
    ) -> Option<ContinuousFutureSegment> {
        if cursor_ns > end_ns || self.transitions.is_empty() {
            return None;
        }

        for (index, row) in self.transitions.iter().enumerate() {
            if cursor_ns < row.transition_time_ns {
                return Some(ContinuousFutureSegment {
                    index,
                    instrument_id: row.pre_instrument_id,
                    start_ns: cursor_ns,
                    end_ns: cmp::min(end_ns, row.transition_time_ns - 1),
                });
            }
        }

        let row = self.transitions.last()?;
        Some(ContinuousFutureSegment {
            index: self.transitions.len(),
            instrument_id: row.post_instrument_id,
            start_ns: cursor_ns,
            end_ns,
        })
    }

    pub(super) fn adjustment_for_segment(&self, segment_index: usize) -> Decimal {
        let is_ratio = self.adjustment_mode.is_ratio();
        let is_backward = self.adjustment_mode.is_backward();
        let mut cumulative = if is_ratio {
            Decimal::ONE
        } else {
            Decimal::ZERO
        };

        let transition_start_index = self
            .first_pre_instrument_id
            .and_then(|instrument_id| {
                self.transitions
                    .iter()
                    .position(|row| row.pre_instrument_id == instrument_id)
            })
            .unwrap_or(0);
        let transition_stop_index = self
            .last_post_instrument_id
            .and_then(|instrument_id| {
                self.transitions
                    .iter()
                    .position(|row| row.post_instrument_id == instrument_id)
                    .map(|index| index + 1)
            })
            .unwrap_or(self.transitions.len());

        let clamped_segment_index = cmp::max(
            transition_start_index,
            cmp::min(segment_index, transition_stop_index),
        );
        let range: Box<dyn Iterator<Item = usize>> = if is_backward {
            Box::new(clamped_segment_index..transition_stop_index)
        } else {
            Box::new(transition_start_index..clamped_segment_index)
        };

        for index in range {
            let row = &self.transitions[index];

            if is_ratio {
                cumulative *= if is_backward {
                    row.post_price / row.pre_price
                } else {
                    row.pre_price / row.post_price
                };
            } else {
                cumulative += if is_backward {
                    row.post_price - row.pre_price
                } else {
                    row.pre_price - row.post_price
                };
            }
        }

        cumulative
    }

    pub(super) fn source_for_segment(
        &self,
        segment_instrument_id: InstrumentId,
    ) -> ContinuousFutureSource {
        let reference_bar_type = if self.primary_bar_type.is_composite() {
            self.primary_bar_type.composite()
        } else {
            self.primary_bar_type
        };

        if reference_bar_type.is_externally_aggregated() {
            let source_bar_type = BarType::new(
                segment_instrument_id,
                reference_bar_type.spec(),
                AggregationSource::External,
            );
            return ContinuousFutureSource::Bars(source_bar_type);
        }

        if reference_bar_type.spec().price_type == PriceType::Last {
            ContinuousFutureSource::Trades
        } else {
            ContinuousFutureSource::Quotes
        }
    }

    pub(super) fn child_params(&self, parent_params: Option<&Params>, parent_id: UUID4) -> Params {
        let mut child_params = parent_params.cloned().unwrap_or_default();
        child_params.shift_remove(CONTINUOUS_FUTURE_TRANSITIONS);
        child_params.shift_remove(CONTINUOUS_FUTURE_ADJUSTMENT_MODE);
        child_params.shift_remove(LAST_POST_INSTRUMENT_ID);
        child_params.shift_remove(FIRST_PRE_INSTRUMENT_ID);
        child_params.shift_remove(BAR_TYPES);
        child_params.insert(
            CONTINUOUS_FUTURE_PARENT_REQUEST_ID.to_string(),
            Value::String(parent_id.to_string()),
        );
        child_params
    }
}

#[derive(Debug, Clone)]
pub(super) struct ContinuousFutureTransition {
    pub(super) transition_time_ns: u64,
    pub(super) pre_instrument_id: InstrumentId,
    pub(super) post_instrument_id: InstrumentId,
    pre_price: Decimal,
    post_price: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinuousFutureSource {
    Bars(BarType),
    Trades,
    Quotes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ContinuousFutureSegment {
    pub(super) index: usize,
    pub(super) instrument_id: InstrumentId,
    pub(super) start_ns: u64,
    pub(super) end_ns: u64,
}

pub(super) fn request_params(req: &RequestCommand) -> Option<&Params> {
    match req {
        RequestCommand::Data(cmd) => cmd.params.as_ref(),
        RequestCommand::Instrument(cmd) => cmd.params.as_ref(),
        RequestCommand::Instruments(cmd) => cmd.params.as_ref(),
        RequestCommand::BookSnapshot(cmd) => cmd.params.as_ref(),
        RequestCommand::BookDeltas(cmd) => cmd.params.as_ref(),
        RequestCommand::BookDepth(cmd) => cmd.params.as_ref(),
        RequestCommand::Quotes(cmd) => cmd.params.as_ref(),
        RequestCommand::Trades(cmd) => cmd.params.as_ref(),
        RequestCommand::FundingRates(cmd) => cmd.params.as_ref(),
        RequestCommand::ForwardPrices(cmd) => cmd.params.as_ref(),
        RequestCommand::Bars(cmd) => cmd.params.as_ref(),
        RequestCommand::Join(cmd) => cmd.params.as_ref(),
    }
}

pub(super) fn response_params(resp: &DataResponse) -> Option<&Params> {
    match resp {
        DataResponse::Data(resp) => resp.params.as_ref(),
        DataResponse::Instrument(resp) => resp.params.as_ref(),
        DataResponse::Instruments(resp) => resp.params.as_ref(),
        DataResponse::Book(resp) => resp.params.as_ref(),
        DataResponse::BookDeltas(resp) => resp.params.as_ref(),
        DataResponse::Quotes(resp) => resp.params.as_ref(),
        DataResponse::Trades(resp) => resp.params.as_ref(),
        DataResponse::FundingRates(resp) => resp.params.as_ref(),
        DataResponse::ForwardPrices(resp) => resp.params.as_ref(),
        DataResponse::Bars(resp) => resp.params.as_ref(),
    }
}

pub(super) fn request_bar_aggregation_from_params(
    params: Option<&Params>,
) -> anyhow::Result<Option<RequestBarAggregation>> {
    let Some(params) = params else {
        return Ok(None);
    };
    let Some(value) = params.get(BAR_TYPES) else {
        return Ok(None);
    };

    let values = value
        .as_array()
        .context("`bar_types` request parameter must be an array")?;
    let mut bar_types = Vec::with_capacity(values.len());
    for value in values {
        let raw = value
            .as_str()
            .context("`bar_types` request parameter must contain strings")?;
        bar_types
            .push(BarType::from_str(raw).context("failed to parse `bar_types` request parameter")?);
    }

    if bar_types.is_empty() {
        return Ok(None);
    }

    if let Some(bar_type) = bar_types
        .iter()
        .find(|bar_type| !bar_type.is_internally_aggregated())
    {
        anyhow::bail!("Cannot request aggregated bars: {bar_type} must be internally aggregated");
    }

    let mut unique_bar_types = Vec::with_capacity(bar_types.len());
    for bar_type in bar_types {
        if !unique_bar_types.contains(&bar_type) {
            unique_bar_types.push(bar_type);
        }
    }

    let update_subscriptions = params.get_bool("update_subscriptions").unwrap_or(false);

    Ok(Some(RequestBarAggregation {
        bar_types: unique_bar_types,
        update_subscriptions,
    }))
}

pub(super) fn has_continuous_future_params(params: Option<&Params>) -> bool {
    params.is_some_and(|params| params.contains_key(CONTINUOUS_FUTURE_TRANSITIONS))
}

pub(super) fn continuous_future_parent_request_id(params: Option<&Params>) -> Option<UUID4> {
    params
        .and_then(|params| params.get_str(CONTINUOUS_FUTURE_PARENT_REQUEST_ID))
        .and_then(|value| UUID4::from_str(value).ok())
}

pub(super) fn continuous_future_request_from_bars(
    request: &RequestBars,
) -> anyhow::Result<Option<ContinuousFutureRequest>> {
    let Some(params) = request.params.as_ref() else {
        return Ok(None);
    };

    if !params.contains_key(CONTINUOUS_FUTURE_TRANSITIONS) {
        return Ok(None);
    }

    let bar_types = continuous_future_bar_types(request, params)?;
    parse_continuous_future(bar_types, params).map(Some)
}

pub(super) fn continuous_future_subscription_from_bars(
    cmd: &SubscribeBars,
) -> anyhow::Result<Option<ContinuousFutureRequest>> {
    let Some(params) = cmd.params.as_ref() else {
        return Ok(None);
    };

    if !params.contains_key(CONTINUOUS_FUTURE_TRANSITIONS) {
        return Ok(None);
    }

    if params.contains_key(BAR_TYPES) {
        anyhow::bail!(
            "Continuous future bar subscriptions must not include `bar_types`; pass the chain in continuous_future_transitions instead"
        );
    }

    parse_continuous_future(vec![cmd.bar_type], params).map(Some)
}

fn parse_continuous_future(
    bar_types: Vec<BarType>,
    params: &Params,
) -> anyhow::Result<ContinuousFutureRequest> {
    let primary_bar_type = bar_types[0];
    if !primary_bar_type.is_internally_aggregated() {
        anyhow::bail!("Continuous future target {primary_bar_type} must be internally aggregated");
    }

    let transitions_value = params
        .get(CONTINUOUS_FUTURE_TRANSITIONS)
        .context("missing `continuous_future_transitions`")?;
    let adjustment_mode = parse_adjustment_mode(params.get(CONTINUOUS_FUTURE_ADJUSTMENT_MODE))
        .with_context(|| {
            format!("Invalid continuous future adjustment mode for {primary_bar_type}")
        })?;
    let first_pre_instrument_id = parse_optional_chain_bound(
        params,
        FIRST_PRE_INSTRUMENT_ID,
        primary_bar_type.instrument_id(),
    )?;
    let last_post_instrument_id = parse_optional_chain_bound(
        params,
        LAST_POST_INSTRUMENT_ID,
        primary_bar_type.instrument_id(),
    )?;
    let transitions = parse_transitions(
        primary_bar_type,
        transitions_value,
        adjustment_mode.is_ratio(),
    )?;

    if transitions.is_empty() {
        anyhow::bail!("Continuous future transitions must not be empty");
    }

    if let Some(instrument_id) = first_pre_instrument_id
        && !transitions
            .iter()
            .any(|row| row.pre_instrument_id == instrument_id)
    {
        anyhow::bail!(
            "Continuous future first_pre_instrument_id {instrument_id} was not found in transitions"
        );
    }

    if let Some(instrument_id) = last_post_instrument_id
        && !transitions
            .iter()
            .any(|row| row.post_instrument_id == instrument_id)
    {
        anyhow::bail!(
            "Continuous future last_post_instrument_id {instrument_id} was not found in transitions"
        );
    }

    Ok(ContinuousFutureRequest {
        primary_bar_type,
        request_bar_aggregation: RequestBarAggregation {
            bar_types,
            update_subscriptions: false,
        },
        transitions,
        adjustment_mode,
        first_pre_instrument_id,
        last_post_instrument_id,
    })
}

fn continuous_future_bar_types(
    request: &RequestBars,
    params: &Params,
) -> anyhow::Result<Vec<BarType>> {
    let Some(value) = params.get(BAR_TYPES) else {
        return Ok(vec![request.bar_type]);
    };

    let values = value
        .as_array()
        .context("`bar_types` request parameter must be an array")?;
    let mut bar_types = Vec::with_capacity(values.len());
    for value in values {
        let raw = value
            .as_str()
            .context("`bar_types` request parameter must contain strings")?;
        let bar_type =
            BarType::from_str(raw).context("failed to parse `bar_types` request parameter")?;

        if !bar_type.is_internally_aggregated() {
            anyhow::bail!(
                "Cannot request aggregated bars: {bar_type} must be internally aggregated"
            );
        }

        if !bar_types.contains(&bar_type) {
            bar_types.push(bar_type);
        }
    }

    if bar_types.is_empty() {
        anyhow::bail!("Continuous future `bar_types` request parameter must not be empty");
    }

    Ok(bar_types)
}

fn parse_adjustment_mode(value: Option<&Value>) -> anyhow::Result<ContinuousFutureAdjustmentType> {
    let Some(value) = value else {
        return Ok(ContinuousFutureAdjustmentType::default());
    };

    if let Some(raw) = value.as_str() {
        return ContinuousFutureAdjustmentType::from_str(raw)
            .with_context(|| format!("failed to parse `{CONTINUOUS_FUTURE_ADJUSTMENT_MODE}`"));
    }

    match value.as_u64() {
        Some(1) => Ok(ContinuousFutureAdjustmentType::BackwardSpread),
        Some(2) => Ok(ContinuousFutureAdjustmentType::ForwardSpread),
        Some(3) => Ok(ContinuousFutureAdjustmentType::BackwardRatio),
        Some(4) => Ok(ContinuousFutureAdjustmentType::ForwardRatio),
        _ => anyhow::bail!("failed to parse `{CONTINUOUS_FUTURE_ADJUSTMENT_MODE}`"),
    }
}

fn parse_optional_chain_bound(
    params: &Params,
    key: &str,
    target_instrument_id: InstrumentId,
) -> anyhow::Result<Option<InstrumentId>> {
    let Some(raw) = params.get_str(key) else {
        return Ok(None);
    };

    let instrument_id = InstrumentId::from_str(raw)
        .with_context(|| format!("Invalid continuous future {key} for {target_instrument_id}"))?;
    if instrument_id.venue != target_instrument_id.venue {
        anyhow::bail!(
            "Continuous future {key} venue mismatch for {target_instrument_id}: target venue {}, {key} venue {}",
            target_instrument_id.venue,
            instrument_id.venue,
        );
    }

    Ok(Some(instrument_id))
}

fn parse_transitions(
    target_bar_type: BarType,
    value: &Value,
    is_ratio: bool,
) -> anyhow::Result<Vec<ContinuousFutureTransition>> {
    let values = value
        .as_array()
        .context("Continuous future transitions must be an array")?;
    let target_venue = target_bar_type.instrument_id().venue;
    let mut transitions = Vec::with_capacity(values.len());
    let mut previous_transition_time_ns = None;
    let mut previous_post_instrument_id = None;

    for row_value in values {
        let row = row_value.as_object().with_context(|| {
            format!("Continuous future transition must be an object, was {row_value}")
        })?;
        let transition_time_ns = row
            .get("transition_time_ns")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("Invalid continuous future transition_time_ns, was {row_value}")
            })?;

        if let Some(previous) = previous_transition_time_ns
            && transition_time_ns <= previous
        {
            anyhow::bail!(
                "Continuous future transition times must be strictly increasing, was {value}"
            );
        }
        previous_transition_time_ns = Some(transition_time_ns);

        let pre_instrument_id =
            parse_transition_instrument_id(row.get("pre_instrument_id"), target_bar_type)?;
        let post_instrument_id =
            parse_transition_instrument_id(row.get("post_instrument_id"), target_bar_type)?;
        if pre_instrument_id.venue != target_venue || post_instrument_id.venue != target_venue {
            anyhow::bail!(
                "Continuous future segment venue mismatch for {target_bar_type}: target venue {target_venue}, segment venues pre={}, post={}",
                pre_instrument_id.venue,
                post_instrument_id.venue,
            );
        }

        if let Some(previous) = previous_post_instrument_id
            && pre_instrument_id != previous
        {
            anyhow::bail!(
                "Continuous future chain discontinuity for {target_bar_type}: previous post {previous} != current pre {pre_instrument_id}",
            );
        }
        previous_post_instrument_id = Some(post_instrument_id);

        let pre_price = parse_transition_price(row.get("pre_price"), row_value, "pre_price")?;
        let post_price = parse_transition_price(row.get("post_price"), row_value, "post_price")?;
        if is_ratio && (pre_price <= Decimal::ZERO || post_price <= Decimal::ZERO) {
            anyhow::bail!(
                "Continuous future ratio adjustment requires positive prices, was {row_value}"
            );
        }

        transitions.push(ContinuousFutureTransition {
            transition_time_ns,
            pre_instrument_id,
            post_instrument_id,
            pre_price,
            post_price,
        });
    }

    Ok(transitions)
}

fn parse_transition_instrument_id(
    value: Option<&Value>,
    target_bar_type: BarType,
) -> anyhow::Result<InstrumentId> {
    let raw = value.and_then(Value::as_str).with_context(|| {
        format!("Continuous future transition missing pre/post_instrument_id for {target_bar_type}")
    })?;
    InstrumentId::from_str(raw).with_context(|| {
        format!("Invalid continuous future transition instrument id for {target_bar_type}")
    })
}

fn parse_transition_price(
    value: Option<&Value>,
    row: &Value,
    key: &str,
) -> anyhow::Result<Decimal> {
    let value =
        value.with_context(|| format!("Continuous future transition missing {key}, was {row}"))?;
    let raw = value
        .as_str()
        .map_or_else(|| value.to_string(), ToString::to_string);
    Decimal::from_str(&raw)
        .with_context(|| format!("Invalid continuous future transition price, was {row}"))
}

#[cfg(test)]
mod tests {
    use nautilus_core::{Params, UnixNanos};
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn request_with_params(params: Params) -> RequestBars {
        RequestBars::new(
            BarType::from("ES.GLBX-1-TICK-LAST-INTERNAL"),
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            Some(params),
        )
    }

    fn transition_params(mode: &str) -> Params {
        serde_json::from_value(json!({
            "continuous_future_adjustment_mode": mode,
            "continuous_future_transitions": [
                {
                    "transition_time_ns": 10,
                    "pre_instrument_id": "ESH24.GLBX",
                    "post_instrument_id": "ESM24.GLBX",
                    "pre_price": "100.00",
                    "post_price": "110.00"
                },
                {
                    "transition_time_ns": 20,
                    "pre_instrument_id": "ESM24.GLBX",
                    "post_instrument_id": "ESU24.GLBX",
                    "pre_price": "120.00",
                    "post_price": "150.00"
                }
            ]
        }))
        .unwrap()
    }

    #[rstest]
    fn test_continuous_future_request_rejects_chain_discontinuity() {
        let params: Params = serde_json::from_value(json!({
            "continuous_future_transitions": [
                {
                    "transition_time_ns": 10,
                    "pre_instrument_id": "ESH24.GLBX",
                    "post_instrument_id": "ESM24.GLBX",
                    "pre_price": "100.00",
                    "post_price": "110.00"
                },
                {
                    "transition_time_ns": 20,
                    "pre_instrument_id": "ESZ24.GLBX",
                    "post_instrument_id": "ESU24.GLBX",
                    "pre_price": "120.00",
                    "post_price": "150.00"
                }
            ]
        }))
        .unwrap();

        let result = continuous_future_request_from_bars(&request_with_params(params));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("chain discontinuity")
        );
    }

    #[rstest]
    fn test_continuous_future_request_rejects_unsorted_transition_times() {
        let params: Params = serde_json::from_value(json!({
            "continuous_future_transitions": [
                {
                    "transition_time_ns": 20,
                    "pre_instrument_id": "ESH24.GLBX",
                    "post_instrument_id": "ESM24.GLBX",
                    "pre_price": "100.00",
                    "post_price": "110.00"
                },
                {
                    "transition_time_ns": 10,
                    "pre_instrument_id": "ESM24.GLBX",
                    "post_instrument_id": "ESU24.GLBX",
                    "pre_price": "120.00",
                    "post_price": "150.00"
                }
            ]
        }))
        .unwrap();

        let result = continuous_future_request_from_bars(&request_with_params(params));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("strictly increasing")
        );
    }

    #[rstest]
    fn test_continuous_future_request_rejects_empty_transitions() {
        let params: Params = serde_json::from_value(json!({
            "continuous_future_transitions": []
        }))
        .unwrap();

        let result = continuous_future_request_from_bars(&request_with_params(params));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must not be empty")
        );
    }

    #[rstest]
    #[case(
        json!({"pre_instrument_id": "ESH24"}),
        json!({}),
        "Invalid continuous future transition instrument id"
    )]
    #[case(
        json!({"transition_time_ns": null}),
        json!({}),
        "Invalid continuous future transition_time_ns"
    )]
    #[case(
        json!({"pre_price": "not-a-price"}),
        json!({}),
        "Invalid continuous future transition price"
    )]
    #[case(
        json!({"post_price": "0"}),
        json!({"continuous_future_adjustment_mode": "FORWARD_RATIO"}),
        "requires positive prices"
    )]
    #[case(
        json!({"pre_price": "0"}),
        json!({"continuous_future_adjustment_mode": "BACKWARD_RATIO"}),
        "requires positive prices"
    )]
    #[case(
        json!({}),
        json!({"last_post_instrument_id": "ESZ24.GLBX"}),
        "last_post_instrument_id"
    )]
    #[case(
        json!({}),
        json!({"first_pre_instrument_id": "ESZ24.GLBX"}),
        "first_pre_instrument_id"
    )]
    fn test_continuous_future_request_rejects_invalid_transition_metadata(
        #[case] transition_overrides: serde_json::Value,
        #[case] params_overrides: serde_json::Value,
        #[case] expected: &str,
    ) {
        let mut transition = json!({
            "transition_time_ns": 10,
            "pre_instrument_id": "ESH24.GLBX",
            "post_instrument_id": "ESM24.GLBX",
            "pre_price": "100.00",
            "post_price": "110.00"
        });
        let transition_row = transition.as_object_mut().unwrap();
        for (key, value) in transition_overrides.as_object().unwrap() {
            transition_row.insert(key.clone(), value.clone());
        }

        let mut params_value = json!({
            "continuous_future_adjustment_mode": "BACKWARD_SPREAD",
            "continuous_future_transitions": [transition]
        });
        let params_row = params_value.as_object_mut().unwrap();
        for (key, value) in params_overrides.as_object().unwrap() {
            params_row.insert(key.clone(), value.clone());
        }
        let params: Params = serde_json::from_value(params_value).unwrap();

        let result = continuous_future_request_from_bars(&request_with_params(params));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(expected));
    }

    #[rstest]
    fn test_continuous_future_next_segment_selects_contract_ranges() {
        let request = continuous_future_request_from_bars(&request_with_params(transition_params(
            "BACKWARD_SPREAD",
        )))
        .unwrap()
        .unwrap();

        assert_eq!(
            request.next_segment(0, 25),
            Some(ContinuousFutureSegment {
                index: 0,
                instrument_id: InstrumentId::from("ESH24.GLBX"),
                start_ns: 0,
                end_ns: 9,
            })
        );
        assert_eq!(
            request.next_segment(10, 25),
            Some(ContinuousFutureSegment {
                index: 1,
                instrument_id: InstrumentId::from("ESM24.GLBX"),
                start_ns: 10,
                end_ns: 19,
            })
        );
        assert_eq!(
            request.next_segment(20, 25),
            Some(ContinuousFutureSegment {
                index: 2,
                instrument_id: InstrumentId::from("ESU24.GLBX"),
                start_ns: 20,
                end_ns: 25,
            })
        );
    }

    #[rstest]
    #[case("BACKWARD_SPREAD", 0, "40.00")]
    #[case("BACKWARD_SPREAD", 1, "30.00")]
    #[case("BACKWARD_SPREAD", 2, "0")]
    #[case("FORWARD_SPREAD", 0, "0")]
    #[case("FORWARD_SPREAD", 1, "-10.00")]
    #[case("FORWARD_SPREAD", 2, "-40.00")]
    fn test_continuous_future_adjustment_for_spread_modes(
        #[case] mode: &str,
        #[case] segment_index: usize,
        #[case] expected: &str,
    ) {
        let request =
            continuous_future_request_from_bars(&request_with_params(transition_params(mode)))
                .unwrap()
                .unwrap();

        assert_eq!(
            request.adjustment_for_segment(segment_index),
            Decimal::from_str(expected).unwrap()
        );
    }

    #[rstest]
    #[case(
        "BACKWARD_SPREAD",
        json!({"last_post_instrument_id": "ESM24.GLBX"}),
        0,
        "10.00"
    )]
    #[case(
        "BACKWARD_SPREAD",
        json!({"last_post_instrument_id": "ESM24.GLBX"}),
        1,
        "0"
    )]
    #[case(
        "FORWARD_SPREAD",
        json!({"first_pre_instrument_id": "ESM24.GLBX"}),
        1,
        "0"
    )]
    #[case(
        "FORWARD_SPREAD",
        json!({"first_pre_instrument_id": "ESM24.GLBX"}),
        2,
        "-30.00"
    )]
    fn test_continuous_future_adjustment_honors_chain_bounds(
        #[case] mode: &str,
        #[case] bound: serde_json::Value,
        #[case] segment_index: usize,
        #[case] expected: &str,
    ) {
        let mut params = transition_params(mode);
        for (key, value) in bound.as_object().unwrap() {
            params.insert(key.clone(), value.clone());
        }
        let request = continuous_future_request_from_bars(&request_with_params(params))
            .unwrap()
            .unwrap();

        assert_eq!(
            request.adjustment_for_segment(segment_index),
            Decimal::from_str(expected).unwrap()
        );
    }

    #[rstest]
    fn test_continuous_future_adjustment_for_backward_ratio() {
        let params: Params = serde_json::from_value(json!({
            "continuous_future_adjustment_mode": "BACKWARD_RATIO",
            "continuous_future_transitions": [
                {
                    "transition_time_ns": 10,
                    "pre_instrument_id": "ESH24.GLBX",
                    "post_instrument_id": "ESM24.GLBX",
                    "pre_price": "100.00",
                    "post_price": "200.00"
                },
                {
                    "transition_time_ns": 20,
                    "pre_instrument_id": "ESM24.GLBX",
                    "post_instrument_id": "ESU24.GLBX",
                    "pre_price": "50.00",
                    "post_price": "150.00"
                }
            ]
        }))
        .unwrap();
        let request = continuous_future_request_from_bars(&request_with_params(params))
            .unwrap()
            .unwrap();

        assert_eq!(request.adjustment_for_segment(0), Decimal::from(6));
        assert_eq!(request.adjustment_for_segment(1), Decimal::from(3));
        assert_eq!(request.adjustment_for_segment(2), Decimal::ONE);
    }

    #[rstest]
    fn test_continuous_future_source_for_external_reference_uses_segment_instrument() {
        let mut params = transition_params("BACKWARD_SPREAD");
        params.insert(
            "bar_types".to_string(),
            json!(["ES.GLBX-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL"]),
        );
        let request = continuous_future_request_from_bars(&request_with_params(params))
            .unwrap()
            .unwrap();

        assert_eq!(
            request.source_for_segment(InstrumentId::from("ESH24.GLBX")),
            ContinuousFutureSource::Bars(BarType::from("ESH24.GLBX-1-MINUTE-LAST-EXTERNAL"))
        );
    }

    #[rstest]
    fn test_continuous_future_source_for_bid_target_uses_quotes() {
        let params: Params = serde_json::from_value(json!({
            "bar_types": ["ES.GLBX-1-TICK-BID-INTERNAL"],
            "continuous_future_transitions": [
                {
                    "transition_time_ns": 10,
                    "pre_instrument_id": "ESH24.GLBX",
                    "post_instrument_id": "ESM24.GLBX",
                    "pre_price": "100.00",
                    "post_price": "110.00"
                }
            ]
        }))
        .unwrap();
        let request = continuous_future_request_from_bars(&request_with_params(params))
            .unwrap()
            .unwrap();

        assert_eq!(
            request.source_for_segment(InstrumentId::from("ESH24.GLBX")),
            ContinuousFutureSource::Quotes
        );
    }
}
