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

use nautilus_model::{
    enums::OmsType,
    identifiers::{InstrumentId, StrategyId},
};
use serde::{Deserialize, Serialize};

/// The base model for all trading strategy configurations.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StrategyConfig {
    /// The unique ID for the strategy. Will become the strategy ID if not None.
    pub strategy_id: Option<StrategyId>,
    /// The unique order ID tag for the strategy. Must be unique
    /// amongst all running strategies for a particular trader ID.
    pub order_id_tag: Option<String>,
    /// If UUID4's should be used for client order ID values.
    #[serde(default = "default_false")]
    pub use_uuid_client_order_ids: bool,
    /// If hyphens should be used in generated client order ID values.
    #[serde(default = "default_true")]
    pub use_hyphens_in_client_order_ids: bool,
    /// The order management system type for the strategy. This will determine
    /// how the `ExecutionEngine` handles position IDs.
    pub oms_type: Option<OmsType>,
    /// The external order claim instrument IDs.
    /// External orders for matching instrument IDs will be associated with (claimed by) the strategy.
    pub external_order_claims: Option<Vec<InstrumentId>>,
    /// If OUO and OCO **open** contingent orders should be managed automatically by the strategy.
    /// Any emulated orders which are active local will be managed by the `OrderEmulator` instead.
    #[serde(default = "default_false")]
    pub manage_contingent_orders: bool,
    /// If all order GTD time in force expirations should be managed by the strategy.
    /// If True, then will ensure open orders have their GTD timers re-activated on start.
    #[serde(default = "default_false")]
    pub manage_gtd_expiry: bool,
    /// If events should be logged by the strategy.
    /// If False, then only warning events and above are logged.
    #[serde(default = "default_true")]
    pub log_events: bool,
    /// If commands should be logged by the strategy.
    #[serde(default = "default_true")]
    pub log_commands: bool,
    /// If order rejected events where `due_post_only` is True should be logged as warnings.
    #[serde(default = "default_true")]
    pub log_rejected_due_post_only_as_warning: bool,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            strategy_id: None,
            order_id_tag: None,
            use_uuid_client_order_ids: false,
            use_hyphens_in_client_order_ids: true,
            oms_type: None,
            external_order_claims: None,
            manage_contingent_orders: false,
            manage_gtd_expiry: false,
            log_events: true,
            log_commands: true,
            log_rejected_due_post_only_as_warning: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_strategy_config_default() {
        let config = StrategyConfig::default();

        assert!(config.strategy_id.is_none());
        assert!(config.order_id_tag.is_none());
        assert!(!config.use_uuid_client_order_ids);
        assert!(config.use_hyphens_in_client_order_ids);
        assert!(config.oms_type.is_none());
        assert!(config.external_order_claims.is_none());
        assert!(!config.manage_contingent_orders);
        assert!(!config.manage_gtd_expiry);
        assert!(config.log_events);
        assert!(config.log_commands);
        assert!(config.log_rejected_due_post_only_as_warning);
    }

    #[rstest]
    fn test_strategy_config_with_strategy_id() {
        let strategy_id = StrategyId::from("TEST-001");
        let config = StrategyConfig {
            strategy_id: Some(strategy_id),
            ..Default::default()
        };

        assert_eq!(config.strategy_id, Some(strategy_id));
    }

    #[rstest]
    fn test_strategy_config_serialization() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("TAG1".to_string()),
            use_uuid_client_order_ids: true,
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: StrategyConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.strategy_id, deserialized.strategy_id);
        assert_eq!(config.order_id_tag, deserialized.order_id_tag);
        assert_eq!(
            config.use_uuid_client_order_ids,
            deserialized.use_uuid_client_order_ids
        );
    }
}
