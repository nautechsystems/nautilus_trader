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

use anyhow::Context;
use ibapi::orders::{
    OrderCondition,
    conditions::{
        ExecutionCondition, MarginCondition, PercentChangeCondition, PriceCondition, TimeCondition,
        TriggerMethod, VolumeCondition,
    },
};
use serde_json::Value;

/// Create IB order conditions from a list of condition dictionaries.
///
/// # Arguments
///
/// * `conditions_data` - A JSON array of condition dictionaries
///
/// # Returns
///
/// A vector of OrderCondition enum variants ready to be encoded into the order.
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
        let condition_type_str = condition_dict
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing condition type")?;

        // Get conjunction (default to "and" = true)
        let conjunction_str = condition_dict
            .get("conjunction")
            .and_then(|v| v.as_str())
            .unwrap_or("and");
        let is_conjunction = conjunction_str.to_lowercase() == "and";

        let condition = match condition_type_str {
            "price" => {
                let con_id = condition_dict
                    .get("conId")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let exchange = condition_dict
                    .get("exchange")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SMART");
                let price = condition_dict
                    .get("price")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let is_more = condition_dict
                    .get("isMore")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let trigger_method = condition_dict
                    .get("triggerMethod")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;

                let mut builder = PriceCondition::builder(con_id, exchange);

                if !is_more {
                    builder = builder.less_than(price);
                } else {
                    builder = builder.greater_than(price);
                }
                builder = builder.trigger_method(TriggerMethod::from(trigger_method));
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Price(builder.build())
            }
            "time" => {
                let time = condition_dict
                    .get("time")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let is_more = condition_dict
                    .get("isMore")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let mut builder = TimeCondition::builder();

                if !is_more {
                    builder = builder.less_than(time);
                } else {
                    builder = builder.greater_than(time);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Time(builder.build())
            }
            "margin" => {
                let percent = condition_dict
                    .get("percent")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let is_more = condition_dict
                    .get("isMore")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let mut builder = MarginCondition::builder();

                if !is_more {
                    builder = builder.less_than(percent);
                } else {
                    builder = builder.greater_than(percent);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Margin(builder.build())
            }
            "execution" => {
                let symbol = condition_dict
                    .get("symbol")
                    .and_then(|v| v.as_str())
                    .context("Missing symbol for execution condition")?;
                let sec_type = condition_dict
                    .get("secType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("STK");
                let exchange = condition_dict
                    .get("exchange")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SMART");

                let mut builder = ExecutionCondition::builder(symbol, sec_type, exchange);
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Execution(builder.build())
            }
            "volume" => {
                let con_id = condition_dict
                    .get("conId")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let exchange = condition_dict
                    .get("exchange")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SMART");
                let volume = condition_dict
                    .get("volume")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let is_more = condition_dict
                    .get("isMore")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let mut builder = VolumeCondition::builder(con_id, exchange);

                if !is_more {
                    builder = builder.less_than(volume);
                } else {
                    builder = builder.greater_than(volume);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::Volume(builder.build())
            }
            "percent_change" => {
                let con_id = condition_dict
                    .get("conId")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let exchange = condition_dict
                    .get("exchange")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SMART");
                let change_percent = condition_dict
                    .get("changePercent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let is_more = condition_dict
                    .get("isMore")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let mut builder = PercentChangeCondition::builder(con_id, exchange);

                if !is_more {
                    builder = builder.less_than(change_percent);
                } else {
                    builder = builder.greater_than(change_percent);
                }
                builder = builder.conjunction(is_conjunction);
                OrderCondition::PercentChange(builder.build())
            }
            _ => {
                tracing::warn!("Unknown condition type: {}", condition_type_str);
                continue;
            }
        };

        conditions.push(condition);
    }

    Ok(conditions)
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
