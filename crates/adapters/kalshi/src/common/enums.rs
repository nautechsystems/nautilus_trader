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

//! Enumerations mirroring the Kalshi Trade API wire values.

use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::common::urls;

/// The Kalshi API environment a client targets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.kalshi",
        eq,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.kalshi")
)]
pub enum KalshiEnvironment {
    /// The demo exchange, which uses its own credentials and balances.
    #[default]
    Demo,
    /// The production exchange, where trades settle real capital.
    Prod,
}

impl KalshiEnvironment {
    /// Returns the REST base URL for this environment.
    #[must_use]
    pub const fn rest_url(&self) -> &'static str {
        match self {
            Self::Demo => urls::DEMO_REST_URL,
            Self::Prod => urls::PROD_REST_URL,
        }
    }

    /// Returns whether this environment trades real capital.
    #[must_use]
    pub const fn is_production(&self) -> bool {
        matches!(self, Self::Prod)
    }
}

impl Display for KalshiEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Demo => f.write_str("demo"),
            Self::Prod => f.write_str("prod"),
        }
    }
}

impl FromStr for KalshiEnvironment {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "demo" => Ok(Self::Demo),
            "prod" | "production" | "live" => Ok(Self::Prod),
            other => Err(format!("Unknown Kalshi environment '{other}'")),
        }
    }
}

/// Identifies the type of a Kalshi market.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiMarketType {
    /// A market whose sides pay the notional value and nothing.
    Binary,
    /// A market that settles at a value on a numeric range.
    Scalar,
}

impl Display for KalshiMarketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Binary => f.write_str("binary"),
            Self::Scalar => f.write_str("scalar"),
        }
    }
}

impl FromStr for KalshiMarketType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "binary" => Ok(Self::Binary),
            "scalar" => Ok(Self::Scalar),
            other => Err(format!("Unknown Kalshi market type '{other}'")),
        }
    }
}

/// The position of a market in its lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiMarketStatus {
    /// Created, not yet open for trading.
    Initialized,
    /// Not open for trading.
    Inactive,
    /// Open for trading.
    Active,
    /// Closed for trading, awaiting determination.
    Closed,
    /// Determined, awaiting settlement.
    Determined,
    /// The determination is contested.
    Disputed,
    /// The determination was amended.
    Amended,
    /// Settled and final.
    Finalized,
}

impl KalshiMarketStatus {
    /// Returns whether the market accepts orders.
    #[must_use]
    pub const fn is_tradable(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns whether the market's outcome is final.
    #[must_use]
    pub const fn is_final(&self) -> bool {
        matches!(self, Self::Finalized)
    }

    /// Returns whether the market's outcome is contested, so automatic settlement must not proceed.
    #[must_use]
    pub const fn is_disputed(&self) -> bool {
        matches!(self, Self::Disputed)
    }

    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Initialized => "initialized",
            Self::Inactive => "inactive",
            Self::Active => "active",
            Self::Closed => "closed",
            Self::Determined => "determined",
            Self::Disputed => "disputed",
            Self::Amended => "amended",
            Self::Finalized => "finalized",
        }
    }
}

impl Display for KalshiMarketStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiMarketStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "initialized" => Ok(Self::Initialized),
            "inactive" => Ok(Self::Inactive),
            "active" => Ok(Self::Active),
            "closed" => Ok(Self::Closed),
            "determined" => Ok(Self::Determined),
            "disputed" => Ok(Self::Disputed),
            "amended" => Ok(Self::Amended),
            "finalized" => Ok(Self::Finalized),
            other => Err(format!("Unknown Kalshi market status '{other}'")),
        }
    }
}

/// The determined outcome of a Kalshi market.
///
/// The exchange publishes an empty value while a market is undetermined, and `scalar` for markets
/// that settle at a value on a range rather than at one of the two sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KalshiMarketResult {
    /// The YES side won and pays the notional value.
    #[serde(rename = "yes")]
    Yes,
    /// The NO side won, so the YES side pays nothing.
    #[serde(rename = "no")]
    No,
    /// The market settles at a value on a range.
    #[serde(rename = "scalar")]
    Scalar,
    /// No outcome has been determined.
    #[default]
    #[serde(rename = "")]
    None,
}

impl KalshiMarketResult {
    /// Returns whether the market has a determined binary outcome.
    #[must_use]
    pub const fn is_binary_outcome(&self) -> bool {
        matches!(self, Self::Yes | Self::No)
    }

    /// Returns whether the YES side of the market won.
    #[must_use]
    pub const fn is_yes(&self) -> bool {
        matches!(self, Self::Yes)
    }

    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Scalar => "scalar",
            Self::None => "",
        }
    }
}

impl Display for KalshiMarketResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiMarketResult {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            "scalar" => Ok(Self::Scalar),
            "" => Ok(Self::None),
            other => Err(format!("Unknown Kalshi market result '{other}'")),
        }
    }
}

/// The outcome side a participant is positioned for.
///
/// `yes` covers buying YES and selling NO; `no` covers buying NO and selling YES.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiOutcomeSide {
    /// Exposure to the YES side of the market.
    Yes,
    /// Exposure to the NO side of the market.
    No,
}

impl Display for KalshiOutcomeSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yes => f.write_str("yes"),
            Self::No => f.write_str("no"),
        }
    }
}

impl FromStr for KalshiOutcomeSide {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            other => Err(format!("Unknown Kalshi outcome side '{other}'")),
        }
    }
}

/// The side of the book, quoted from the YES leg.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiBookSide {
    /// Buy YES.
    Bid,
    /// Sell YES, which is economically equivalent to buying NO at one minus the price.
    Ask,
}

impl Display for KalshiBookSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bid => f.write_str("bid"),
            Self::Ask => f.write_str("ask"),
        }
    }
}

impl FromStr for KalshiBookSide {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "bid" => Ok(Self::Bid),
            "ask" => Ok(Self::Ask),
            other => Err(format!("Unknown Kalshi book side '{other}'")),
        }
    }
}

/// The side of a market an order is placed on.
///
/// The adapter models a Kalshi market as one instrument quoted from its YES side, so every order it
/// sends carries [`KalshiOrderSide::Yes`]. The NO side exists on the wire for orders and fills
/// placed outside the adapter, which still have to be reported correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiOrderSide {
    /// The YES side of the market.
    Yes,
    /// The NO side of the market.
    No,
}

impl KalshiOrderSide {
    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
        }
    }

    /// Returns whether the side is the YES side.
    #[must_use]
    pub const fn is_yes(&self) -> bool {
        matches!(self, Self::Yes)
    }
}

impl Display for KalshiOrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiOrderSide {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            other => Err(format!("Unknown Kalshi order side '{other}'")),
        }
    }
}

/// Whether an order buys or sells the side it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiOrderAction {
    /// Buys the named side.
    Buy,
    /// Sells the named side.
    Sell,
}

impl KalshiOrderAction {
    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }

    /// Returns whether the action buys.
    #[must_use]
    pub const fn is_buy(&self) -> bool {
        matches!(self, Self::Buy)
    }
}

impl Display for KalshiOrderAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiOrderAction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            other => Err(format!("Unknown Kalshi order action '{other}'")),
        }
    }
}

/// The order types the exchange accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiOrderType {
    /// An order that rests at a price until it trades or expires.
    Limit,
    /// An order that trades at the best available prices.
    Market,
}

impl KalshiOrderType {
    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Limit => "limit",
            Self::Market => "market",
        }
    }
}

impl Display for KalshiOrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiOrderType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "limit" => Ok(Self::Limit),
            "market" => Ok(Self::Market),
            other => Err(format!("Unknown Kalshi order type '{other}'")),
        }
    }
}

/// How long an order remains open.
///
/// The exchange has no explicit expiration instruction: a resting order expires at the timestamp
/// carried in the request's `expiration_ts`, so [`KalshiTimeInForce::GoodTillCanceled`] covers both
/// a good-till-canceled and a good-till-date order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiTimeInForce {
    /// Trade the whole quantity or cancel.
    FillOrKill,
    /// Rest until canceled or expired.
    GoodTillCanceled,
    /// Trade what is available and cancel the rest.
    ImmediateOrCancel,
}

impl KalshiTimeInForce {
    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::FillOrKill => "fill_or_kill",
            Self::GoodTillCanceled => "good_till_canceled",
            Self::ImmediateOrCancel => "immediate_or_cancel",
        }
    }
}

impl Display for KalshiTimeInForce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiTimeInForce {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "fill_or_kill" => Ok(Self::FillOrKill),
            "good_till_canceled" => Ok(Self::GoodTillCanceled),
            "immediate_or_cancel" => Ok(Self::ImmediateOrCancel),
            other => Err(format!("Unknown Kalshi time in force '{other}'")),
        }
    }
}

/// The lifecycle state the exchange reports for an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiOrderStatus {
    /// Accepted and not yet in the book.
    Pending,
    /// Working in the book.
    Resting,
    /// Canceled, either by the member or by the exchange.
    Canceled,
    /// Fully traded.
    Executed,
}

impl KalshiOrderStatus {
    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resting => "resting",
            Self::Canceled => "canceled",
            Self::Executed => "executed",
        }
    }

    /// Returns whether the exchange will no longer change the order.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Canceled | Self::Executed)
    }
}

impl Display for KalshiOrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiOrderStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "resting" | "live" => Ok(Self::Resting),
            "canceled" | "cancelled" => Ok(Self::Canceled),
            "executed" | "filled" => Ok(Self::Executed),
            other => Err(format!("Unknown Kalshi order status '{other}'")),
        }
    }
}

/// The self-trade prevention the exchange applies to an order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.kalshi",
        eq,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.kalshi")
)]
pub enum KalshiSelfTradePrevention {
    /// Trade against the member's own resting order as the taker.
    #[default]
    TakerAtCross,
    /// Rest instead of trading against the member's own order.
    Maker,
}

impl KalshiSelfTradePrevention {
    /// Returns the Kalshi wire value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::TakerAtCross => "taker_at_cross",
            Self::Maker => "maker",
        }
    }
}

impl Display for KalshiSelfTradePrevention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KalshiSelfTradePrevention {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "taker_at_cross" => Ok(Self::TakerAtCross),
            "maker" => Ok(Self::Maker),
            other => Err(format!(
                "Unknown Kalshi self trade prevention type '{other}'"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("initialized", KalshiMarketStatus::Initialized)]
    #[case("inactive", KalshiMarketStatus::Inactive)]
    #[case("active", KalshiMarketStatus::Active)]
    #[case("closed", KalshiMarketStatus::Closed)]
    #[case("determined", KalshiMarketStatus::Determined)]
    #[case("disputed", KalshiMarketStatus::Disputed)]
    #[case("amended", KalshiMarketStatus::Amended)]
    #[case("finalized", KalshiMarketStatus::Finalized)]
    fn test_market_status_round_trips_wire_values(
        #[case] wire: &str,
        #[case] expected: KalshiMarketStatus,
    ) {
        assert_eq!(wire.parse::<KalshiMarketStatus>().unwrap(), expected);
        assert_eq!(expected.as_str(), wire);
        assert_eq!(format!("{expected}"), wire);
    }

    #[rstest]
    fn test_market_status_rejects_unknown_value() {
        let error = "settled".parse::<KalshiMarketStatus>().unwrap_err();

        assert!(error.contains("Unknown Kalshi market status"), "{error}");
    }

    #[rstest]
    fn test_market_status_lifecycle_helpers() {
        assert!(KalshiMarketStatus::Active.is_tradable());
        assert!(!KalshiMarketStatus::Closed.is_tradable());
        assert!(KalshiMarketStatus::Finalized.is_final());
        assert!(KalshiMarketStatus::Disputed.is_disputed());
        assert!(!KalshiMarketStatus::Determined.is_final());
    }

    #[rstest]
    #[case(r#""yes""#, KalshiMarketResult::Yes)]
    #[case(r#""no""#, KalshiMarketResult::No)]
    #[case(r#""scalar""#, KalshiMarketResult::Scalar)]
    #[case(r#""""#, KalshiMarketResult::None)]
    fn test_market_result_deserializes_wire_values(
        #[case] raw: &str,
        #[case] expected: KalshiMarketResult,
    ) {
        let parsed: KalshiMarketResult = serde_json::from_str(raw).unwrap();

        assert_eq!(parsed, expected);
    }

    #[rstest]
    fn test_market_result_reports_binary_outcomes() {
        assert!(KalshiMarketResult::Yes.is_binary_outcome());
        assert!(KalshiMarketResult::Yes.is_yes());
        assert!(KalshiMarketResult::No.is_binary_outcome());
        assert!(!KalshiMarketResult::No.is_yes());
        assert!(!KalshiMarketResult::Scalar.is_binary_outcome());
        assert!(!KalshiMarketResult::None.is_binary_outcome());
        assert_eq!(KalshiMarketResult::default(), KalshiMarketResult::None);
    }

    #[rstest]
    #[case("demo", KalshiEnvironment::Demo)]
    #[case("PROD", KalshiEnvironment::Prod)]
    #[case("live", KalshiEnvironment::Prod)]
    fn test_environment_parses_aliases(#[case] raw: &str, #[case] expected: KalshiEnvironment) {
        assert_eq!(raw.parse::<KalshiEnvironment>().unwrap(), expected);
    }

    #[rstest]
    fn test_environment_defaults_to_demo_and_never_reaches_production_implicitly() {
        let environment = KalshiEnvironment::default();

        assert_eq!(environment, KalshiEnvironment::Demo);
        assert!(!environment.is_production());
        assert_eq!(environment.rest_url(), urls::DEMO_REST_URL);
        assert_eq!(KalshiEnvironment::Prod.rest_url(), urls::PROD_REST_URL);
    }

    #[rstest]
    fn test_market_type_and_sides_round_trip() {
        assert_eq!(
            "binary".parse::<KalshiMarketType>().unwrap(),
            KalshiMarketType::Binary
        );
        assert_eq!(
            "scalar".parse::<KalshiMarketType>().unwrap(),
            KalshiMarketType::Scalar
        );
        assert_eq!(
            "yes".parse::<KalshiOutcomeSide>().unwrap(),
            KalshiOutcomeSide::Yes
        );
        assert_eq!(
            "no".parse::<KalshiOutcomeSide>().unwrap(),
            KalshiOutcomeSide::No
        );
        assert_eq!(
            "bid".parse::<KalshiBookSide>().unwrap(),
            KalshiBookSide::Bid
        );
        assert_eq!(
            "ask".parse::<KalshiBookSide>().unwrap(),
            KalshiBookSide::Ask
        );
    }

    #[rstest]
    #[case("yes", KalshiOrderSide::Yes)]
    #[case("no", KalshiOrderSide::No)]
    fn test_order_side_round_trips_wire_values(
        #[case] wire: &str,
        #[case] expected: KalshiOrderSide,
    ) {
        assert_eq!(wire.parse::<KalshiOrderSide>().unwrap(), expected);
        assert_eq!(expected.as_str(), wire);
        assert_eq!(format!("{expected}"), wire);
        assert_eq!(expected.is_yes(), expected == KalshiOrderSide::Yes);
    }

    #[rstest]
    #[case("buy", KalshiOrderAction::Buy)]
    #[case("sell", KalshiOrderAction::Sell)]
    fn test_order_action_round_trips_wire_values(
        #[case] wire: &str,
        #[case] expected: KalshiOrderAction,
    ) {
        assert_eq!(wire.parse::<KalshiOrderAction>().unwrap(), expected);
        assert_eq!(expected.as_str(), wire);
        assert_eq!(format!("{expected}"), wire);
        assert_eq!(expected.is_buy(), expected == KalshiOrderAction::Buy);
    }

    #[rstest]
    #[case("limit", KalshiOrderType::Limit)]
    #[case("market", KalshiOrderType::Market)]
    fn test_order_type_round_trips_wire_values(
        #[case] wire: &str,
        #[case] expected: KalshiOrderType,
    ) {
        assert_eq!(wire.parse::<KalshiOrderType>().unwrap(), expected);
        assert_eq!(expected.as_str(), wire);
    }

    #[rstest]
    #[case("fill_or_kill", KalshiTimeInForce::FillOrKill)]
    #[case("good_till_canceled", KalshiTimeInForce::GoodTillCanceled)]
    #[case("immediate_or_cancel", KalshiTimeInForce::ImmediateOrCancel)]
    fn test_time_in_force_round_trips_wire_values(
        #[case] wire: &str,
        #[case] expected: KalshiTimeInForce,
    ) {
        assert_eq!(wire.parse::<KalshiTimeInForce>().unwrap(), expected);
        assert_eq!(expected.as_str(), wire);
    }

    #[rstest]
    #[case("pending", KalshiOrderStatus::Pending)]
    #[case("resting", KalshiOrderStatus::Resting)]
    #[case("canceled", KalshiOrderStatus::Canceled)]
    #[case("executed", KalshiOrderStatus::Executed)]
    fn test_order_status_round_trips_wire_values(
        #[case] wire: &str,
        #[case] expected: KalshiOrderStatus,
    ) {
        assert_eq!(wire.parse::<KalshiOrderStatus>().unwrap(), expected);
        assert_eq!(expected.as_str(), wire);
        assert_eq!(format!("{expected}"), wire);
    }

    #[rstest]
    fn test_order_status_terminal_states() {
        assert!(!KalshiOrderStatus::Pending.is_terminal());
        assert!(!KalshiOrderStatus::Resting.is_terminal());
        assert!(KalshiOrderStatus::Canceled.is_terminal());
        assert!(KalshiOrderStatus::Executed.is_terminal());
    }

    #[rstest]
    fn test_order_enums_reject_unknown_values() {
        assert!(
            "maybe"
                .parse::<KalshiOrderSide>()
                .unwrap_err()
                .contains("order side")
        );
        assert!(
            "hold"
                .parse::<KalshiOrderAction>()
                .unwrap_err()
                .contains("order action")
        );
        assert!(
            "stop"
                .parse::<KalshiOrderType>()
                .unwrap_err()
                .contains("order type")
        );
        assert!(
            "good_till_date"
                .parse::<KalshiTimeInForce>()
                .unwrap_err()
                .contains("time in force")
        );
        assert!(
            "partially_filled"
                .parse::<KalshiOrderStatus>()
                .unwrap_err()
                .contains("order status")
        );
    }
}
