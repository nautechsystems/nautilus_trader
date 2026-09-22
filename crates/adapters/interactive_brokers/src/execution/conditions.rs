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

//! Order conditions implementation for Interactive Brokers conditional orders.

use std::str::FromStr;

use anyhow::Context;
use ibapi::orders::{
    OrderCondition,
    conditions::{
        ExecutionCondition, MarginCondition, PercentChangeCondition, PriceCondition, TimeCondition,
        VolumeCondition,
    },
};
use serde_json::Value;

use crate::common::enums::{
    IbConditionConjunction, IbConditionKind, IbSecurityType, IbTriggerMethod,
};

/// Create IB order conditions from a list of condition dictionaries.
///
/// # Errors
///
/// Returns an error if conditions_data is not an array or if any condition is invalid.
pub fn create_ib_conditions(conditions_data: &Value) -> anyhow::Result<Vec<OrderCondition>> {
    let conditions_array = conditions_data
        .as_array()
        .context("Conditions must be an array")?;

    let mut conditions = Vec::new();

    for condition_dict in conditions_array {
        let condition_type = json_str(condition_dict, "type").context("Missing condition type")?;
        let condition_kind = match IbConditionKind::from_str(condition_type) {
            Ok(condition_kind) => condition_kind,
            Err(_) => {
                tracing::warn!("Unknown IB condition type: {condition_type}, skipping condition");
                continue;
            }
        };

        // Get conjunction (default to "and" = true)
        let conjunction = json_str(condition_dict, "conjunction")
            .map(IbConditionConjunction::from_str)
            .transpose()?
            .unwrap_or(IbConditionConjunction::And);
        let is_conjunction = conjunction.is_conjunction();

        let condition = match condition_kind {
            IbConditionKind::Price => {
                let con_id = json_i32(condition_dict, "conId").unwrap_or(0);
                let exchange = json_str(condition_dict, "exchange").unwrap_or("SMART");
                let price = json_f64(condition_dict, "price").unwrap_or(0.0);
                let is_more = json_bool(condition_dict, "isMore").unwrap_or(true);
                let trigger_method = json_i32(condition_dict, "triggerMethod").unwrap_or(0);

                let mut builder = PriceCondition::builder(con_id, exchange);

                if is_more {
                    builder = builder.greater_than(price);
                } else {
                    builder = builder.less_than(price);
                }
                builder = builder.trigger_method(
                    IbTriggerMethod::try_from(trigger_method)?.ibapi_trigger_method(),
                );
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Price(builder.build())
            }
            IbConditionKind::Time => {
                let time = json_str(condition_dict, "time").unwrap_or("");
                let is_more = json_bool(condition_dict, "isMore").unwrap_or(true);

                let mut builder = TimeCondition::builder();

                if is_more {
                    builder = builder.greater_than(time);
                } else {
                    builder = builder.less_than(time);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Time(builder.build())
            }
            IbConditionKind::Margin => {
                let percent = json_i32(condition_dict, "percent").unwrap_or(0);
                let is_more = json_bool(condition_dict, "isMore").unwrap_or(true);

                let mut builder = MarginCondition::builder();

                if is_more {
                    builder = builder.greater_than(percent);
                } else {
                    builder = builder.less_than(percent);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Margin(builder.build())
            }
            IbConditionKind::Execution => {
                let symbol = json_str(condition_dict, "symbol")
                    .context("Missing symbol for execution condition")?;
                let sec_type = json_str(condition_dict, "secType").unwrap_or("STK");
                let sec_type = IbSecurityType::from_str(sec_type)
                    .map_or_else(|_| sec_type.to_string(), |sec_type| sec_type.to_string());
                let exchange = json_str(condition_dict, "exchange").unwrap_or("SMART");

                let mut builder = ExecutionCondition::builder(symbol, sec_type.as_str(), exchange);
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Execution(builder.build())
            }
            IbConditionKind::Volume => {
                let con_id = json_i32(condition_dict, "conId").unwrap_or(0);
                let exchange = json_str(condition_dict, "exchange").unwrap_or("SMART");
                let volume = json_i32(condition_dict, "volume").unwrap_or(0);
                let is_more = json_bool(condition_dict, "isMore").unwrap_or(true);

                let mut builder = VolumeCondition::builder(con_id, exchange);

                if is_more {
                    builder = builder.greater_than(volume);
                } else {
                    builder = builder.less_than(volume);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Volume(builder.build())
            }
            IbConditionKind::PercentChange => {
                let con_id = json_i32(condition_dict, "conId").unwrap_or(0);
                let exchange = json_str(condition_dict, "exchange").unwrap_or("SMART");
                let change_percent = json_f64(condition_dict, "changePercent").unwrap_or(0.0);
                let is_more = json_bool(condition_dict, "isMore").unwrap_or(true);

                let mut builder = PercentChangeCondition::builder(con_id, exchange);

                if is_more {
                    builder = builder.greater_than(change_percent);
                } else {
                    builder = builder.less_than(change_percent);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::PercentChange(builder.build())
            }
        };

        conditions.push(condition);
    }

    Ok(conditions)
}

fn json_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn json_i32(value: &Value, key: &str) -> Option<i32> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .map(|value| value as i32)
}

fn json_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn json_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

#[cfg(test)]
mod tests {
    use ibapi::orders::OrderCondition;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_create_conditions_from_json() {
        let conditions_json = serde_json::json!([
            {
                "type": "price",
                "conId": 265598,
                "exchange": "SMART",
                "isMore": true,
                "price": 250.0,
                "triggerMethod": 0,
                "conjunction": "and",
            },
            {
                "type": "time",
                "time": "20250315-09:30:00",
                "isMore": true,
                "conjunction": "or",
            },
        ]);

        let conditions = create_ib_conditions(&conditions_json).unwrap();
        assert_eq!(conditions.len(), 2);
        assert_eq!(conditions[0].condition_type(), 1); // Price condition type
        assert_eq!(conditions[1].condition_type(), 3); // Time condition type
        assert!(conditions[0].is_conjunction()); // "and"
        assert!(!conditions[1].is_conjunction()); // "or"
    }

    #[rstest]
    fn test_create_execution_condition_from_json() {
        let conditions_json = serde_json::json!([
            {
                "type": "execution",
                "symbol": "MSFT",
                "secType": "STK",
                "exchange": "SMART",
                "conjunction": "or",
            }
        ]);

        let conditions = create_ib_conditions(&conditions_json).unwrap();
        assert_eq!(conditions.len(), 1);

        match &conditions[0] {
            OrderCondition::Execution(condition) => {
                assert_eq!(condition.symbol, "MSFT");
                assert_eq!(condition.security_type, "STK");
                assert_eq!(condition.exchange, "SMART");
                assert!(!condition.is_conjunction);
            }
            other => panic!("unexpected condition: {other:?}"),
        }
    }

    #[rstest]
    fn test_create_percent_change_condition_from_json() {
        let conditions_json = serde_json::json!([
            {
                "type": "percent_change",
                "conId": 123,
                "exchange": "NASDAQ",
                "changePercent": 2.5,
                "isMore": false,
                "conjunction": "and",
            }
        ]);

        let conditions = create_ib_conditions(&conditions_json).unwrap();
        assert_eq!(conditions.len(), 1);

        match &conditions[0] {
            OrderCondition::PercentChange(condition) => {
                assert_eq!(condition.contract_id, 123);
                assert_eq!(condition.exchange, "NASDAQ");
                assert_eq!(condition.percent, 2.5);
                assert!(!condition.is_more);
                assert!(condition.is_conjunction);
            }
            other => panic!("unexpected condition: {other:?}"),
        }
    }

    #[rstest]
    fn test_create_conditions_skips_unknown_type() {
        let conditions_json = serde_json::json!([
            {
                "type": "unknown",
            },
            {
                "type": "margin",
                "percent": 25,
                "isMore": true,
            }
        ]);

        let conditions = create_ib_conditions(&conditions_json).unwrap();
        assert_eq!(conditions.len(), 1);
        assert_eq!(conditions[0].condition_type(), 4);
    }

    #[rstest]
    fn test_create_conditions_rejects_non_array_json() {
        let conditions_json = serde_json::json!({
            "type": "price",
            "price": 123.0,
        });

        let result = create_ib_conditions(&conditions_json);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Conditions must be an array"
        );
    }
}
