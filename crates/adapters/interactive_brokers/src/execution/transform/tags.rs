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

use std::str::FromStr;

use anyhow::Context;
use ibapi::{contracts::TagValue, orders::Order as IBOrder};
use serde_json::{Map, Value};
use ustr::Ustr;

use crate::{
    common::enums::{IbOrderType, IbTimeInForce},
    execution::conditions::create_ib_conditions,
};

pub(super) fn apply_ib_order_tags(
    ib_order: &mut IBOrder,
    tags: Option<&[Ustr]>,
) -> anyhow::Result<()> {
    let Some(tags) = tags else {
        return Ok(());
    };

    for tag in tags {
        let tag_str = tag.as_str();
        if !tag_str.starts_with("IBOrderTags:") {
            continue;
        }

        let json_str = tag_str.trim_start_matches("IBOrderTags:");
        let tags_obj = serde_json::from_str::<Value>(json_str)
            .with_context(|| format!("Invalid IBOrderTags JSON: {json_str}"))?;
        apply_ib_order_tag_json(ib_order, &tags_obj)?;
    }

    Ok(())
}

fn apply_ib_order_tag_json(ib_order: &mut IBOrder, tags_obj: &Value) -> anyhow::Result<()> {
    let Some(tags_map) = tags_obj.as_object() else {
        anyhow::bail!("Invalid IBOrderTags payload: expected a JSON object");
    };

    let mut updated_order = ib_order.clone();
    apply_ib_order_tag_overlay(&mut updated_order, tags_map)?;
    apply_ib_order_conditions(&mut updated_order, tags_obj)?;

    *ib_order = updated_order;
    Ok(())
}

fn apply_ib_order_tag_overlay(
    ib_order: &mut IBOrder,
    tags_map: &Map<String, Value>,
) -> anyhow::Result<()> {
    let mut order_value = serde_json::to_value(&*ib_order)
        .context("Failed to serialize IB order before applying IBOrderTags")?;

    for (key, value) in tags_map {
        let field = canonical_order_tag_key(key);
        if should_skip_generic_overlay(&field) {
            continue;
        }

        if field == "non_guaranteed" {
            apply_non_guaranteed_combo_tag(ib_order, value)?;
            sync_order_field(&mut order_value, "smart_combo_routing_params", ib_order)?;
            continue;
        }

        let Some(mut updates) = normalize_order_tag_update(&field, value) else {
            anyhow::bail!("Invalid IBOrderTags field {field}: {value}");
        };

        for (field, value) in updates.drain(..) {
            apply_order_field_update(ib_order, &mut order_value, &field, value)?;
        }
    }

    Ok(())
}

fn apply_order_field_update(
    ib_order: &mut IBOrder,
    order_value: &mut Value,
    field: &str,
    value: Value,
) -> anyhow::Result<()> {
    let Some(order_obj) = order_value.as_object_mut() else {
        anyhow::bail!("Failed to apply IBOrderTags because IB order JSON is not an object");
    };

    if !order_obj.contains_key(field) {
        anyhow::bail!("Unsupported IBOrderTags field: {field}");
    }

    let previous = order_obj.insert(field.to_string(), value);

    match serde_json::from_value::<IBOrder>(order_value.clone()) {
        Ok(updated_order) => {
            *ib_order = updated_order;
            Ok(())
        }
        Err(e) => {
            let Some(order_obj) = order_value.as_object_mut() else {
                anyhow::bail!(
                    "Failed to restore IB order JSON after invalid IBOrderTags field: {field}"
                );
            };

            match previous {
                Some(previous) => order_obj.insert(field.to_string(), previous),
                None => order_obj.remove(field),
            };
            Err(anyhow::anyhow!("Invalid IBOrderTags field {field}: {e}"))
        }
    }
}

fn sync_order_field(
    order_value: &mut Value,
    field: &str,
    ib_order: &IBOrder,
) -> anyhow::Result<()> {
    let updated_order_value = serde_json::to_value(ib_order).with_context(|| {
        format!("Failed to serialize IB order after applying IBOrderTags field: {field}")
    })?;

    let Some(updated_field_value) = updated_order_value.get(field).cloned() else {
        return Ok(());
    };

    if let Some(order_obj) = order_value.as_object_mut() {
        order_obj.insert(field.to_string(), updated_field_value);
    }
    Ok(())
}

fn normalize_order_tag_update(field: &str, value: &Value) -> Option<Vec<(String, Value)>> {
    match field {
        "order_type" => normalize_order_type_update(value),
        "tif" => normalize_tif_value(value)
            .map(|value| vec![("tif".to_string(), value)])
            .or_else(|| warn_invalid_tag_value("tif", value)),
        "active_start_time" | "active_stop_time" | "auto_cancel_date" | "good_after_time"
        | "good_till_date" | "manual_order_time" => normalize_utc_datetime_value(field, value),
        "action" => normalize_string_enum_value(
            "action",
            value,
            &[
                ("BUY", "Buy"),
                ("SELL", "Sell"),
                ("SSHORT", "SellShort"),
                ("SLONG", "SellLong"),
            ],
        ),
        "oca_type" => normalize_i32_enum_value(
            "oca_type",
            value,
            &[
                (0, "None"),
                (1, "CancelWithBlock"),
                (2, "ReduceWithBlock"),
                (3, "ReduceWithoutBlock"),
            ],
        ),
        "trigger_method" => normalize_i32_enum_value(
            "trigger_method",
            value,
            &[
                (0, "Default"),
                (1, "DoubleBidAsk"),
                (2, "Last"),
                (3, "DoubleLast"),
                (4, "BidAsk"),
                (7, "LastOrBidAsk"),
                (8, "Midpoint"),
            ],
        ),
        "origin" => normalize_i32_enum_value("origin", value, &[(0, "Customer"), (1, "Firm")]),
        "short_sale_slot" => normalize_i32_enum_value(
            "short_sale_slot",
            value,
            &[(0, "None"), (1, "Broker"), (2, "ThirdParty")],
        ),
        "auction_strategy" => normalize_i32_enum_value(
            "auction_strategy",
            value,
            &[(1, "Match"), (2, "Improvement"), (3, "Transparent")],
        ),
        "volatility_type" => {
            normalize_i32_enum_value("volatility_type", value, &[(1, "Daily"), (2, "Annual")])
        }
        "reference_price_type" => normalize_i32_enum_value(
            "reference_price_type",
            value,
            &[(1, "AverageOfNBBO"), (2, "NBBO")],
        ),
        "rule_80_a" => normalize_string_enum_value(
            "rule_80_a",
            value,
            &[
                ("I", "Individual"),
                ("A", "Agency"),
                ("W", "AgentOtherMember"),
                ("J", "IndividualPTIA"),
                ("U", "AgencyPTIA"),
                ("M", "AgentOtherMemberPTIA"),
                ("K", "IndividualPT"),
                ("Y", "AgencyPT"),
                ("N", "AgentOtherMemberPT"),
            ],
        ),
        "open_close" => {
            normalize_string_enum_value("open_close", value, &[("O", "Open"), ("C", "Close")])
        }
        _ => Some(vec![(field.to_string(), value.clone())]),
    }
}

fn normalize_utc_datetime_value(field: &str, value: &Value) -> Option<Vec<(String, Value)>> {
    let Some(value) = value.as_str() else {
        return warn_invalid_tag_value(field, value);
    };

    if value.is_empty() || value.ends_with(" UTC") {
        return Some(vec![(field.to_string(), Value::String(value.to_string()))]);
    }

    tracing::warn!(
        "Ignoring invalid IBOrderTags value for {field}: expected '%Y%m%d %H:%M:%S UTC', received {value}"
    );
    None
}

fn normalize_order_type_update(value: &Value) -> Option<Vec<(String, Value)>> {
    let Some(order_type) = value.as_str() else {
        return warn_invalid_tag_value("order_type", value);
    };

    let normalized = normalize_tag_value(order_type);
    match normalized.as_str() {
        "MARKETONOPEN" => Some(vec![
            order_type_value(IbOrderType::Market),
            tif_value(IbTimeInForce::OnOpen),
        ]),
        "LIMITONOPEN" => Some(vec![
            order_type_value(IbOrderType::Limit),
            tif_value(IbTimeInForce::OnOpen),
        ]),
        "ATAUCTION" => Some(vec![
            order_type_value(IbOrderType::MarketToLimit),
            tif_value(IbTimeInForce::Auction),
        ]),
        "AUCTIONLIMIT" | "COMBOLIMIT" => Some(vec![order_type_value(IbOrderType::Limit)]),
        "AUCTIONRELATIVE" => Some(vec![order_type_value(IbOrderType::Relative)]),
        "COMBOMARKET" => Some(vec![order_type_value(IbOrderType::Market)]),
        _ => match parse_ib_order_type(order_type) {
            Some(parsed) => Some(vec![order_type_value(parsed)]),
            None => {
                tracing::warn!("Ignoring unsupported IB orderType override: {order_type}");
                None
            }
        },
    }
}

fn order_type_value(order_type: IbOrderType) -> (String, Value) {
    (
        "order_type".to_string(),
        Value::String(order_type.as_str().to_string()),
    )
}

fn tif_value(tif: IbTimeInForce) -> (String, Value) {
    ("tif".to_string(), ibapi_tif_serde_value(tif))
}

fn normalize_tif_value(value: &Value) -> Option<Value> {
    let tif = value.as_str()?;
    parse_ib_tif(tif).map(ibapi_tif_serde_value)
}

fn ibapi_tif_serde_value(tif: IbTimeInForce) -> Value {
    let value = match tif {
        IbTimeInForce::Day => "Day",
        IbTimeInForce::GoodTilCanceled => "GoodTilCanceled",
        IbTimeInForce::ImmediateOrCancel => "ImmediateOrCancel",
        IbTimeInForce::GoodTilDate => "GoodTilDate",
        IbTimeInForce::OnOpen => "OnOpen",
        IbTimeInForce::FillOrKill => "FillOrKill",
        IbTimeInForce::DayTilCanceled => "DayTilCanceled",
        IbTimeInForce::Auction => "Auction",
    };

    Value::String(value.to_string())
}

fn normalize_i32_enum_value(
    field: &str,
    value: &Value,
    variants: &[(i64, &str)],
) -> Option<Vec<(String, Value)>> {
    if value.is_null() {
        return Some(vec![(field.to_string(), Value::Null)]);
    }

    if let Some(value) = value.as_i64() {
        return variants
            .iter()
            .find_map(|(raw, name)| (*raw == value).then_some(enum_update(field, name)))
            .or_else(|| warn_invalid_tag_value(field, &Value::from(value)));
    }

    let Some(value) = value.as_str() else {
        return warn_invalid_tag_value(field, value);
    };

    let normalized = normalize_tag_value(value);
    variants
        .iter()
        .find_map(|(raw, name)| {
            let raw = raw.to_string();
            (normalized == normalize_tag_value(name) || normalized == raw)
                .then_some(enum_update(field, name))
        })
        .or_else(|| warn_invalid_tag_value(field, &Value::String(value.to_string())))
}

fn normalize_string_enum_value(
    field: &str,
    value: &Value,
    variants: &[(&str, &str)],
) -> Option<Vec<(String, Value)>> {
    if value.is_null() {
        return Some(vec![(field.to_string(), Value::Null)]);
    }

    let Some(value) = value.as_str() else {
        return warn_invalid_tag_value(field, value);
    };

    let normalized = normalize_tag_value(value);
    variants
        .iter()
        .find_map(|(wire, name)| {
            (normalized == normalize_tag_value(wire) || normalized == normalize_tag_value(name))
                .then_some(enum_update(field, name))
        })
        .or_else(|| warn_invalid_tag_value(field, &Value::String(value.to_string())))
}

fn enum_update(field: &str, variant: &str) -> Vec<(String, Value)> {
    vec![(field.to_string(), Value::String(variant.to_string()))]
}

fn warn_invalid_tag_value<T>(field: &str, value: &Value) -> Option<T> {
    tracing::warn!("Ignoring invalid IBOrderTags value for {field}: {value}");
    None
}

fn apply_ib_order_conditions(ib_order: &mut IBOrder, tags_obj: &Value) -> anyhow::Result<()> {
    let Some(conditions_array) = tags_obj.get("conditions").and_then(|v| v.as_array()) else {
        return Ok(());
    };

    if conditions_array.is_empty() {
        return Ok(());
    }

    match create_ib_conditions(&Value::Array(conditions_array.clone())) {
        Ok(conditions) => {
            if conditions.is_empty() {
                return Ok(());
            }

            ib_order.conditions = conditions;
            tracing::debug!("Setting {} conditions on order", ib_order.conditions.len());

            if let Some(conditions_cancel_order) = tags_obj
                .get("conditionsCancelOrder")
                .and_then(|v| v.as_bool())
            {
                ib_order.conditions_cancel_order = conditions_cancel_order;
            }
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Invalid IBOrderTags conditions: {e}")),
    }
}

fn parse_ib_order_type(value: &str) -> Option<IbOrderType> {
    let upper = value.to_ascii_uppercase();
    IbOrderType::from_str(value)
        .or_else(|_| IbOrderType::from_str(&upper))
        .ok()
}

fn parse_ib_tif(value: &str) -> Option<IbTimeInForce> {
    let upper = value.to_ascii_uppercase();
    IbTimeInForce::from_str(value)
        .or_else(|_| IbTimeInForce::from_str(&upper))
        .ok()
}

fn apply_non_guaranteed_combo_tag(ib_order: &mut IBOrder, value: &Value) -> anyhow::Result<()> {
    let Some(non_guaranteed) = parse_bool_like(value) else {
        anyhow::bail!("Invalid IBOrderTags value for NonGuaranteed: {value}");
    };

    set_tag_value(
        &mut ib_order.smart_combo_routing_params,
        "NonGuaranteed",
        if non_guaranteed { "1" } else { "0" },
    );
    Ok(())
}

fn parse_bool_like(value: &Value) -> Option<bool> {
    if let Some(value) = value.as_bool() {
        return Some(value);
    }

    if let Some(value) = value.as_i64() {
        return match value {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
    }

    match value.as_str()?.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn set_tag_value(target: &mut Vec<TagValue>, tag: &str, value: &str) {
    if let Some(existing) = target.iter_mut().find(|tag_value| tag_value.tag == tag) {
        existing.value = value.to_string();
        return;
    }

    push_tag_value(target, tag, value);
}

fn should_skip_generic_overlay(field: &str) -> bool {
    matches!(field, "conditions")
}

fn canonical_order_tag_key(key: &str) -> String {
    match key {
        "timeInForce" | "time_in_force" => "tif".to_string(),
        "orderType" => "order_type".to_string(),
        "rule80A" | "rule_80A" | "Rule80A" => "rule_80_a".to_string(),
        "NonGuaranteed" => "non_guaranteed".to_string(),
        _ => lower_camel_or_pascal_to_snake(key),
    }
}

fn lower_camel_or_pascal_to_snake(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::with_capacity(value.len());

    for (index, ch) in chars.iter().enumerate() {
        if *ch == '-' {
            result.push('_');
            continue;
        }

        if ch.is_ascii_uppercase() {
            let prev = index.checked_sub(1).and_then(|prev| chars.get(prev));
            let next = chars.get(index + 1);
            let needs_separator = prev.is_some_and(|prev| {
                (prev.is_ascii_lowercase() || prev.is_ascii_digit())
                    || (prev.is_ascii_uppercase()
                        && next.is_some_and(|next| next.is_ascii_lowercase()))
            });

            if needs_separator {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(*ch);
        }
    }

    result
}

fn normalize_tag_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn push_tag_value(target: &mut Vec<TagValue>, tag: &str, value: &str) {
    target.push(TagValue {
        tag: tag.to_string(),
        value: value.to_string(),
    });
}
