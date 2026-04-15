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

use ibapi::{
    contracts::TagValue,
    orders::{OcaType, Order as IBOrder},
};
use serde_json::Value;
use ustr::Ustr;

use crate::execution::conditions::create_ib_conditions;

pub(super) fn apply_ib_order_tags(ib_order: &mut IBOrder, tags: Option<&[Ustr]>) {
    let Some(tags) = tags else {
        return;
    };

    for tag in tags {
        let tag_str = tag.as_str();
        if !tag_str.starts_with("IBOrderTags:") {
            continue;
        }

        let json_str = tag_str.trim_start_matches("IBOrderTags:");
        if let Ok(tags_obj) = serde_json::from_str::<Value>(json_str) {
            apply_ib_order_tag_json(ib_order, &tags_obj);
        }
    }
}

fn apply_ib_order_tag_json(ib_order: &mut IBOrder, tags_obj: &Value) {
    if let Some(hidden) = tags_obj.get("hidden").and_then(|v| v.as_bool()) {
        ib_order.hidden = hidden;
    }

    if let Some(aon) = tags_obj.get("allOrNone").and_then(|v| v.as_bool()) {
        ib_order.all_or_none = aon;
    }

    if let Some(min_qty) = tags_obj.get("minQty").and_then(|v| v.as_i64()) {
        ib_order.min_qty = Some(min_qty as i32);
    }

    if let Some(oca_group) = tags_obj.get("ocaGroup").and_then(|v| v.as_str())
        && !oca_group.is_empty()
    {
        ib_order.oca_group = oca_group.to_string();
    }

    if let Some(oca_type) = tags_obj.get("ocaType").and_then(|v| v.as_i64()) {
        ib_order.oca_type = OcaType::from(oca_type as i32);
    }

    if let Some(outside_rth) = tags_obj.get("outsideRth").and_then(|v| v.as_bool()) {
        ib_order.outside_rth = outside_rth;
    }

    if let Some(what_if) = tags_obj.get("whatIf").and_then(|v| v.as_bool()) {
        ib_order.what_if = what_if;
        if what_if {
            tracing::debug!("whatIf flag enabled - order will be analyzed but not submitted");
        }
    }

    apply_ib_order_time_tags(ib_order, tags_obj);
    apply_ib_order_array_tags(ib_order, tags_obj);
    apply_ib_order_conditions(ib_order, tags_obj);

    if let Some(block_order) = tags_obj.get("blockOrder").and_then(|v| v.as_bool()) {
        ib_order.block_order = block_order;
    }

    if let Some(sweep_to_fill) = tags_obj.get("sweepToFill").and_then(|v| v.as_bool()) {
        ib_order.sweep_to_fill = sweep_to_fill;
    }
}

fn apply_ib_order_time_tags(ib_order: &mut IBOrder, tags_obj: &Value) {
    if let Some(active_start_time) = tags_obj.get("activeStartTime").and_then(|v| v.as_str())
        && !active_start_time.is_empty()
    {
        push_tag_value(
            &mut ib_order.order_misc_options,
            "activeStartTime",
            active_start_time,
        );
    }

    if let Some(active_stop_time) = tags_obj.get("activeStopTime").and_then(|v| v.as_str())
        && !active_stop_time.is_empty()
    {
        push_tag_value(
            &mut ib_order.order_misc_options,
            "activeStopTime",
            active_stop_time,
        );
    }

    if let Some(good_after_time) = tags_obj.get("goodAfterTime").and_then(|v| v.as_str())
        && !good_after_time.is_empty()
    {
        ib_order.good_after_time = good_after_time.to_string();
    }
}

fn apply_ib_order_array_tags(ib_order: &mut IBOrder, tags_obj: &Value) {
    if let Some(algo_params) = tags_obj.get("algoParams").and_then(|v| v.as_array()) {
        ib_order.algo_params = tag_values_from_json(algo_params);
    }

    if let Some(order_misc_options) = tags_obj.get("orderMiscOptions").and_then(|v| v.as_array()) {
        ib_order.order_misc_options = tag_values_from_json(order_misc_options);
    }

    if let Some(smart_combo_routing_params) = tags_obj
        .get("smartComboRoutingParams")
        .and_then(|v| v.as_array())
    {
        ib_order.smart_combo_routing_params = tag_values_from_json(smart_combo_routing_params);
    }
}

fn apply_ib_order_conditions(ib_order: &mut IBOrder, tags_obj: &Value) {
    let Some(conditions_array) = tags_obj.get("conditions").and_then(|v| v.as_array()) else {
        return;
    };

    if conditions_array.is_empty() {
        return;
    }

    match create_ib_conditions(&Value::Array(conditions_array.clone())) {
        Ok(conditions) => {
            if conditions.is_empty() {
                return;
            }

            ib_order.conditions = conditions;
            tracing::debug!("Setting {} conditions on order", ib_order.conditions.len());

            if let Some(conditions_cancel_order) = tags_obj
                .get("conditionsCancelOrder")
                .and_then(|v| v.as_bool())
            {
                ib_order.conditions_cancel_order = conditions_cancel_order;
            }
        }
        Err(e) => tracing::warn!("Failed to create conditions: {e}"),
    }
}

fn push_tag_value(target: &mut Vec<TagValue>, tag: &str, value: &str) {
    target.push(TagValue {
        tag: tag.to_string(),
        value: value.to_string(),
    });
}

fn tag_values_from_json(values: &[Value]) -> Vec<TagValue> {
    values
        .iter()
        .filter_map(|value| {
            Some(TagValue {
                tag: value.get("tag")?.as_str()?.to_string(),
                value: value.get("value")?.as_str()?.to_string(),
            })
        })
        .collect()
}
