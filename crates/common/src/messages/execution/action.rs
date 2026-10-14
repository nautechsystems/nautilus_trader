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

//! Commands and delivery results for adapter-owned execution actions.

use std::fmt::Display;

use derive_builder::Builder;
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use serde::{Deserialize, Serialize};
use strum::{Display as StrumDisplay, EnumString};

/// The delivery outcome of an [`ExecuteAction`] command.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, StrumDisplay, EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        module = "nautilus_trader.live",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.live")
)]
pub enum ExecutionActionStatus {
    /// The venue returned a definitive success response.
    Accepted,
    /// The command was not sent or the venue explicitly rejected it.
    Rejected,
    /// The request may have reached the venue, but no definitive response exists.
    #[default]
    Ambiguous,
}

/// The canonical Yes and No instruments for one binary-option outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryOptionPair {
    /// The Yes outcome instrument identifier.
    pub yes_instrument_id: InstrumentId,
    /// The No outcome instrument identifier.
    pub no_instrument_id: InstrumentId,
}

impl BinaryOptionPair {
    /// Creates a new [`BinaryOptionPair`].
    ///
    /// # Errors
    ///
    /// Returns an error if both legs use the same instrument identifier.
    pub fn new(
        yes_instrument_id: InstrumentId,
        no_instrument_id: InstrumentId,
    ) -> anyhow::Result<Self> {
        let pair = Self {
            yes_instrument_id,
            no_instrument_id,
        };
        pair.validate()?;
        Ok(pair)
    }

    /// Validates the pair's common structural invariants.
    ///
    /// # Errors
    ///
    /// Returns an error if both legs use the same instrument identifier.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.yes_instrument_id != self.no_instrument_id,
            "Binary option Yes and No instrument IDs must differ"
        );
        Ok(())
    }
}

/// A binary-option inventory conversion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum BinaryOptionAction {
    /// Converts quote collateral into equal Yes and No outcome inventory.
    Split {
        /// The canonical Yes and No outcome pair.
        pair: BinaryOptionPair,
        /// The conversion quantity.
        quantity: Quantity,
        /// Venue-specific conversion options.
        params: Option<Params>,
    },
    /// Converts equal Yes and No outcome inventory into quote collateral.
    Merge {
        /// The canonical Yes and No outcome pair.
        pair: BinaryOptionPair,
        /// The conversion quantity.
        quantity: Quantity,
        /// Venue-specific conversion options.
        params: Option<Params>,
    },
}

impl BinaryOptionAction {
    /// Returns the canonical action name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Split { .. } => "binary_option.split",
            Self::Merge { .. } => "binary_option.merge",
        }
    }

    /// Returns the binary-option pair.
    #[must_use]
    pub const fn pair(&self) -> BinaryOptionPair {
        match self {
            Self::Split { pair, .. } | Self::Merge { pair, .. } => *pair,
        }
    }

    /// Returns the venue-specific conversion options.
    #[must_use]
    pub const fn params(&self) -> Option<&Params> {
        match self {
            Self::Split { params, .. } | Self::Merge { params, .. } => params.as_ref(),
        }
    }

    /// Returns the conversion quantity.
    #[must_use]
    pub const fn quantity(&self) -> Quantity {
        match self {
            Self::Split { quantity, .. } | Self::Merge { quantity, .. } => *quantity,
        }
    }

    /// Validates the action's common structural invariants.
    ///
    /// # Errors
    ///
    /// Returns an error if the pair is malformed or the quantity is not positive.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.pair().validate()?;
        anyhow::ensure!(
            self.quantity().is_positive(),
            "Binary option action quantity must be positive"
        );
        Ok(())
    }
}

/// A strategy-owned execution operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "action", rename_all = "snake_case")]
pub enum ExecutionAction {
    /// A standard binary-option conversion.
    BinaryOption(BinaryOptionAction),
}

impl ExecutionAction {
    /// Returns the canonical action name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::BinaryOption(action) => action.name(),
        }
    }

    /// Returns the venue-specific action parameters.
    #[must_use]
    pub const fn params(&self) -> Option<&Params> {
        match self {
            Self::BinaryOption(action) => action.params(),
        }
    }

    /// Returns the binary-option action, if applicable.
    #[must_use]
    pub const fn binary_option(&self) -> Option<&BinaryOptionAction> {
        match self {
            Self::BinaryOption(action) => Some(action),
        }
    }

    /// Validates the action's common structural invariants.
    ///
    /// # Errors
    ///
    /// Returns an error if the concrete action is malformed.
    pub fn validate(&self) -> anyhow::Result<()> {
        match self {
            Self::BinaryOption(action) => action.validate(),
        }
    }
}

impl From<BinaryOptionAction> for ExecutionAction {
    fn from(action: BinaryOptionAction) -> Self {
        Self::BinaryOption(action)
    }
}

impl Display for ExecutionAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A strategy-owned request for an execution action.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, Builder)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct ExecuteAction {
    pub trader_id: TraderId,
    pub client_id: Option<ClientId>,
    pub strategy_id: StrategyId,
    pub account_id: AccountId,
    pub action: ExecutionAction,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    #[builder(default)]
    pub correlation_id: Option<UUID4>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<UUID4>,
}

impl ExecuteAction {
    /// Creates a new [`ExecuteAction`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: Option<ClientId>,
        strategy_id: StrategyId,
        account_id: AccountId,
        action: ExecutionAction,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            strategy_id,
            account_id,
            action,
            command_id,
            ts_init,
            correlation_id,
            causation_id: None,
        }
    }
}

impl Display for ExecuteAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ExecuteAction(client_id={:?}, account_id={}, action={})",
            self.client_id,
            self.account_id,
            self.action.name(),
        )
    }
}

/// The correlated delivery result for an [`ExecuteAction`] command.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, Builder)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen, from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct ExecutionActionResult {
    pub trader_id: TraderId,
    pub client_id: Option<ClientId>,
    pub strategy_id: StrategyId,
    pub account_id: AccountId,
    pub action: String,
    pub command_id: UUID4,
    pub status: ExecutionActionStatus,
    pub reason: Option<String>,
    pub info: Option<Params>,
    pub ts_event: UnixNanos,
}

impl ExecutionActionResult {
    /// Creates a new result for `command`.
    #[must_use]
    pub fn new(
        command: &ExecuteAction,
        status: ExecutionActionStatus,
        reason: Option<String>,
        info: Option<Params>,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            trader_id: command.trader_id,
            client_id: command.client_id,
            strategy_id: command.strategy_id,
            account_id: command.account_id,
            action: command.action.name().to_string(),
            command_id: command.command_id,
            status,
            reason,
            info,
            ts_event,
        }
    }
}

impl Display for ExecutionActionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ExecutionActionResult(command_id={}, action={}, status={})",
            self.command_id, self.action, self.status,
        )
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_result_retains_command_identity() {
        let command = ExecuteAction::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("HYPERLIQUID")),
            StrategyId::from("STRATEGY-001"),
            AccountId::from("HYPERLIQUID-001"),
            ExecutionAction::BinaryOption(BinaryOptionAction::Split {
                pair: BinaryOptionPair::new(
                    InstrumentId::from("42-YES-OUTCOME.HYPERLIQUID"),
                    InstrumentId::from("42-NO-OUTCOME.HYPERLIQUID"),
                )
                .unwrap(),
                quantity: Quantity::from("1.25"),
                params: None,
            }),
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            UnixNanos::from(1),
            None,
        );

        let result = ExecutionActionResult::new(
            &command,
            ExecutionActionStatus::Accepted,
            None,
            None,
            UnixNanos::from(2),
        );

        assert_eq!(result.command_id, command.command_id);
        assert_eq!(result.strategy_id, command.strategy_id);
        assert_eq!(result.status, ExecutionActionStatus::Accepted);
    }

    #[rstest]
    fn test_binary_option_action_validation_rejects_untrusted_invalid_payloads() {
        let duplicate_pair = BinaryOptionPair {
            yes_instrument_id: InstrumentId::from("42-YES-OUTCOME.HYPERLIQUID"),
            no_instrument_id: InstrumentId::from("42-YES-OUTCOME.HYPERLIQUID"),
        };
        let action = BinaryOptionAction::Split {
            pair: duplicate_pair,
            quantity: Quantity::from("1.25"),
            params: None,
        };

        let error = action.validate().expect_err("invalid action must reject");

        assert_eq!(
            error.to_string(),
            "Binary option Yes and No instrument IDs must differ"
        );

        let zero_quantity_action = BinaryOptionAction::Merge {
            pair: BinaryOptionPair::new(
                InstrumentId::from("42-YES-OUTCOME.HYPERLIQUID"),
                InstrumentId::from("42-NO-OUTCOME.HYPERLIQUID"),
            )
            .unwrap(),
            quantity: Quantity::from("0"),
            params: None,
        };
        let error = zero_quantity_action
            .validate()
            .expect_err("zero quantity must reject");

        assert_eq!(
            error.to_string(),
            "Binary option action quantity must be positive"
        );
    }
}
