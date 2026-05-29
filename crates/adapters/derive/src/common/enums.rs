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

//! Wire-format enums for the Derive REST/WS APIs.
//!
//! Variants are sourced from the venue's own Rust SDK at
//! [`derivexyz/cockpit`](https://github.com/derivexyz/cockpit/tree/master/orderbook-types/src),
//! which is generated from Derive's JSON-RPC schemas. Variant wire strings are
//! case-sensitive and must round-trip byte-equivalent.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Derive network selector. Drives REST/WS URLs and per-network protocol
/// constants (`DOMAIN_SEPARATOR`, module addresses).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.derive",
        eq,
        eq_int,
        frozen,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.derive")
)]
pub enum DeriveEnvironment {
    /// Production environment.
    #[default]
    Mainnet,
    /// Public testnet environment.
    Testnet,
}

impl DeriveEnvironment {
    #[must_use]
    pub const fn is_testnet(self) -> bool {
        matches!(self, Self::Testnet)
    }
}

/// Wire-level instrument type returned by `public/get_instruments` and used as
/// the `instrument_type` filter on listing endpoints and WS channel names like
/// `trades.{instrument_type}.{currency}`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    AsRefStr,
    EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveInstrumentType {
    /// ERC-20 spot asset.
    Erc20,
    /// Option contract.
    Option,
    /// Perpetual swap.
    Perp,
}

/// Wire-level asset type as returned in subaccount and collateral responses
/// (`asset_type` field on `private/get_subaccount`, `public/get_assets`,
/// `private/get_collaterals`). The variants line up byte-for-byte with
/// [`DeriveInstrumentType`] but the venue treats it as a distinct field.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveAssetType {
    /// ERC-20 collateral asset.
    Erc20,
    /// Option position asset.
    Option,
    /// Perpetual position asset.
    Perp,
}

/// Order side (`direction` field on the venue API).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveOrderSide {
    Buy,
    Sell,
}

/// Order type accepted by `private/order`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveOrderType {
    Limit,
    Market,
}

/// Order lifecycle status reported by `private/get_orders`,
/// `private/get_order_history`, and the WS `{subaccount_id}.orders` channel.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveOrderStatus {
    /// Resting in the book or accepted by the matching engine.
    Open,
    /// Fully filled.
    Filled,
    /// Rejected by matching engine or risk checks.
    Rejected,
    /// Cancelled by user, system, or expiry; see `cancel_reason`.
    Cancelled,
    /// `signature_expiry_sec` was reached.
    Expired,
}

/// Time-in-force flag accepted by `private/order`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeriveTimeInForce {
    /// Good-till-cancelled.
    Gtc,
    /// Post-only: reject if it would cross the spread.
    PostOnly,
    /// Fill-or-kill: fill in full immediately or cancel.
    Fok,
    /// Immediate-or-cancel: fill any tradable quantity, cancel the rest.
    Ioc,
}

/// Option kind parsed from the `instrument_name` suffix (e.g. `-C` or `-P`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum DeriveOptionKind {
    /// Call option.
    #[serde(rename = "C")]
    #[strum(serialize = "C")]
    Call,
    /// Put option.
    #[serde(rename = "P")]
    #[strum(serialize = "P")]
    Put,
}

/// Cancel reason attached to a cancelled order. Empty string corresponds to no
/// reason (order is still open or finished by another transition).
///
/// `ValidationFailed` is present in the SDK enum but absent from the public
/// JSON schema; included here for round-trip fidelity against SDK responses.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum DeriveOrderCancelReason {
    /// No cancel reason (open or finished without cancellation).
    #[serde(rename = "")]
    #[strum(serialize = "")]
    Empty,
    /// Explicit cancel via `private/cancel*`.
    #[serde(rename = "user_request")]
    #[strum(serialize = "user_request")]
    UserRequest,
    /// Market Maker Protection tripped.
    #[serde(rename = "mmp_trigger")]
    #[strum(serialize = "mmp_trigger")]
    MmpTrigger,
    /// Margin check failed at matching.
    #[serde(rename = "insufficient_margin")]
    #[strum(serialize = "insufficient_margin")]
    InsufficientMargin,
    /// Signed `max_fee` is below the venue's current required fee.
    #[serde(rename = "signed_max_fee_too_low")]
    #[strum(serialize = "signed_max_fee_too_low")]
    SignedMaxFeeTooLow,
    /// WS disconnect cancelled the session with cancel-on-disconnect enabled.
    #[serde(rename = "cancel_on_disconnect")]
    #[strum(serialize = "cancel_on_disconnect")]
    CancelOnDisconnect,
    /// Remainder of an IOC or market order auto-cancelled.
    #[serde(rename = "ioc_or_market_partial_fill")]
    #[strum(serialize = "ioc_or_market_partial_fill")]
    IocOrMarketPartialFill,
    /// Signing session key was deregistered.
    #[serde(rename = "session_key_deregistered")]
    #[strum(serialize = "session_key_deregistered")]
    SessionKeyDeregistered,
    /// Subaccount fully withdrawn.
    #[serde(rename = "subaccount_withdrawn")]
    #[strum(serialize = "subaccount_withdrawn")]
    SubaccountWithdrawn,
    /// Cancelled by compliance action.
    #[serde(rename = "compliance")]
    #[strum(serialize = "compliance")]
    Compliance,
    /// Pre-engine validation failure (SDK-only, not in the public schema).
    #[serde(rename = "validation_failed")]
    #[strum(serialize = "validation_failed")]
    ValidationFailed,
    /// Post-only order would cross the market.
    #[serde(rename = "Post only order cannot cross the market")]
    #[strum(serialize = "Post only order cannot cross the market")]
    PostOnlyCrossMarket,
}

/// Cancel reason attached to a cancelled RFQ quote. Similar to
/// [`DeriveOrderCancelReason`] but emits `rfq_no_longer_open` instead of
/// `ioc_or_market_partial_fill`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum DeriveRfqCancelReason {
    /// No cancel reason.
    #[serde(rename = "")]
    #[strum(serialize = "")]
    Empty,
    /// Explicit cancel via `private/cancel_quote`.
    #[serde(rename = "user_request")]
    #[strum(serialize = "user_request")]
    UserRequest,
    /// Margin check failed at quote execution.
    #[serde(rename = "insufficient_margin")]
    #[strum(serialize = "insufficient_margin")]
    InsufficientMargin,
    /// Signed `max_fee` is below the venue's current required fee.
    #[serde(rename = "signed_max_fee_too_low")]
    #[strum(serialize = "signed_max_fee_too_low")]
    SignedMaxFeeTooLow,
    /// MMP tripped.
    #[serde(rename = "mmp_trigger")]
    #[strum(serialize = "mmp_trigger")]
    MmpTrigger,
    /// WS disconnect cancelled the session with cancel-on-disconnect enabled.
    #[serde(rename = "cancel_on_disconnect")]
    #[strum(serialize = "cancel_on_disconnect")]
    CancelOnDisconnect,
    /// Signing session key was deregistered.
    #[serde(rename = "session_key_deregistered")]
    #[strum(serialize = "session_key_deregistered")]
    SessionKeyDeregistered,
    /// Subaccount fully withdrawn.
    #[serde(rename = "subaccount_withdrawn")]
    #[strum(serialize = "subaccount_withdrawn")]
    SubaccountWithdrawn,
    /// Underlying RFQ was filled or cancelled.
    #[serde(rename = "rfq_no_longer_open")]
    #[strum(serialize = "rfq_no_longer_open")]
    RfqNoLongerOpen,
    /// Cancelled by compliance action.
    #[serde(rename = "compliance")]
    #[strum(serialize = "compliance")]
    Compliance,
}

/// Role of the user in a trade (`liquidity_role` field).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveLiquidityRole {
    /// Resting side of the trade.
    Maker,
    /// Aggressing side of the trade.
    Taker,
}

/// Blockchain transaction lifecycle status (`tx_status` field on trades,
/// deposits, withdrawals, transfers, and liquidation history).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveTxStatus {
    /// Transaction queued, not yet broadcast.
    Requested,
    /// Broadcast on-chain, awaiting confirmation.
    Pending,
    /// Confirmed and applied.
    Settled,
    /// Reverted on-chain.
    Reverted,
    /// Superseded or dropped without being applied.
    Ignored,
}

/// Subaccount margining mode. Returned by `private/get_subaccount`. Subaccount
/// creation accepts only [`Sm`](Self::Sm) and [`Pm`](Self::Pm); `Pm2` appears
/// only as a read value when an account has migrated to PMRM v2.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum DeriveMarginType {
    /// Standard Margin.
    #[serde(rename = "SM")]
    #[strum(serialize = "SM")]
    Sm,
    /// Portfolio Margin (legacy PMRM).
    #[serde(rename = "PM")]
    #[strum(serialize = "PM")]
    Pm,
    /// Portfolio Margin v2 (PMRM_2); read-only on `private/get_subaccount`.
    #[serde(rename = "PM2")]
    #[strum(serialize = "PM2")]
    Pm2,
}

/// Liquidation auction type returned by `private/get_liquidation_history`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveAuctionType {
    /// Auction on a still-solvent account.
    Solvent,
    /// Auction on an insolvent account.
    Insolvent,
}

/// Liquidation auction state.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveAuctionState {
    /// Auction is active.
    Ongoing,
    /// Auction has concluded.
    Ended,
}

/// Notification acknowledgement state on `private/get_notifications` and
/// `private/update_notifications`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveNotificationStatus {
    /// Not yet viewed.
    Unseen,
    /// Viewed but not dismissed.
    Seen,
    /// Dismissed from feed.
    Hidden,
}

/// Notification category. The `Types` variant is emitted by the upstream
/// codegen and surfaces in the wire enum; treat it as a generic placeholder
/// rather than a meaningful category.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveNotificationType {
    /// Deposit event.
    Deposit,
    /// Withdrawal event.
    Withdraw,
    /// ERC-20 transfer between subaccounts.
    Transfer,
    /// Trade execution.
    Trade,
    /// Option or perp settlement.
    Settlement,
    /// Liquidation event.
    Liquidation,
    /// Codegen placeholder; not a stable category.
    Types,
}

/// Cause of a balance row update on the WS `{subaccount_id}.balances`
/// channel.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeriveBalanceUpdateType {
    /// Balance changed because of a trade fill.
    Trade,
    /// Asset deposited into the subaccount.
    AssetDeposit,
    /// Asset withdrawn from the subaccount.
    AssetWithdrawal,
    /// Internal transfer between subaccounts.
    Transfer,
    /// Subaccount-level collateral deposit.
    SubaccountDeposit,
    /// Subaccount-level collateral withdrawal.
    SubaccountWithdrawal,
    /// Balance change from liquidation.
    Liquidation,
    /// Reconciliation against on-chain state.
    OnchainDriftFix,
    /// Perpetual funding or mark settlement.
    PerpSettlement,
    /// Option expiry settlement.
    OptionSettlement,
    /// Interest credit or debit.
    InterestAccrual,
    /// Earlier balance change reverted on-chain.
    OnchainRevert,
    /// Revert of a revert.
    DoubleRevert,
}

/// Compliance status returned by `public/change_compliance_status`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeriveComplianceStatus {
    /// Compliance restrictions on.
    Enabled,
    /// Compliance restrictions off.
    Disabled,
}

/// Discrete depth values allowed in the WS subscription channel name
/// `orderbook.{instrument_name}.{group}.{depth}`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum DeriveOrderbookDepth {
    /// Top 1 level per side.
    #[serde(rename = "1")]
    #[strum(serialize = "1")]
    D1,
    /// Top 10 levels per side.
    #[serde(rename = "10")]
    #[strum(serialize = "10")]
    D10,
    /// Top 20 levels per side.
    #[serde(rename = "20")]
    #[strum(serialize = "20")]
    D20,
    /// Top 100 levels per side.
    #[serde(rename = "100")]
    #[strum(serialize = "100")]
    D100,
}

/// Discrete price-grouping values for `orderbook.{instrument_name}.{group}.{depth}`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum DeriveOrderbookGroup {
    /// No grouping.
    #[serde(rename = "1")]
    #[strum(serialize = "1")]
    G1,
    /// Group prices to 10x the tick.
    #[serde(rename = "10")]
    #[strum(serialize = "10")]
    G10,
    /// Group prices to 100x the tick.
    #[serde(rename = "100")]
    #[strum(serialize = "100")]
    G100,
}

/// Discrete ticker push intervals (milliseconds) for the WS
/// `ticker_slim.{instrument_name}.{interval}` channel.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
pub enum DeriveTickerInterval {
    /// 100ms ticker updates.
    #[serde(rename = "100")]
    #[strum(serialize = "100")]
    Ms100,
    /// 1000ms ticker updates.
    #[serde(rename = "1000")]
    #[strum(serialize = "1000")]
    Ms1000,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_environment_default_is_mainnet() {
        assert_eq!(DeriveEnvironment::default(), DeriveEnvironment::Mainnet);
        assert!(!DeriveEnvironment::default().is_testnet());
        assert!(DeriveEnvironment::Testnet.is_testnet());
    }

    #[rstest]
    #[case(DeriveInstrumentType::Erc20, "erc20")]
    #[case(DeriveInstrumentType::Option, "option")]
    #[case(DeriveInstrumentType::Perp, "perp")]
    fn test_instrument_type_display(#[case] variant: DeriveInstrumentType, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
    }

    #[rstest]
    #[case(DeriveTimeInForce::Gtc, "gtc")]
    #[case(DeriveTimeInForce::PostOnly, "post_only")]
    #[case(DeriveTimeInForce::Fok, "fok")]
    #[case(DeriveTimeInForce::Ioc, "ioc")]
    fn test_time_in_force_wire_strings(#[case] variant: DeriveTimeInForce, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveTimeInForce::from_str(expected).unwrap(), variant);
        assert_eq!(
            serde_json::to_string(&variant).unwrap(),
            format!("\"{expected}\""),
        );
    }

    #[rstest]
    fn test_option_kind_serialization_uses_single_letter() {
        assert_eq!(DeriveOptionKind::Call.to_string(), "C");
        assert_eq!(DeriveOptionKind::Put.to_string(), "P");
        assert_eq!(
            DeriveOptionKind::from_str("C").unwrap(),
            DeriveOptionKind::Call
        );
        assert_eq!(
            DeriveOptionKind::from_str("P").unwrap(),
            DeriveOptionKind::Put
        );
    }

    #[rstest]
    #[case(DeriveAssetType::Erc20, "erc20")]
    #[case(DeriveAssetType::Option, "option")]
    #[case(DeriveAssetType::Perp, "perp")]
    fn test_asset_type_wire_strings(#[case] variant: DeriveAssetType, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveAssetType::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveOrderSide::Buy, "buy")]
    #[case(DeriveOrderSide::Sell, "sell")]
    fn test_order_side_wire_strings(#[case] variant: DeriveOrderSide, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(
            serde_json::from_str::<DeriveOrderSide>(&format!("\"{expected}\"")).unwrap(),
            variant
        );
    }

    #[rstest]
    #[case(DeriveOrderType::Limit, "limit")]
    #[case(DeriveOrderType::Market, "market")]
    fn test_order_type_wire_strings(#[case] variant: DeriveOrderType, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveOrderType::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveOrderStatus::Open, "open")]
    #[case(DeriveOrderStatus::Filled, "filled")]
    #[case(DeriveOrderStatus::Rejected, "rejected")]
    #[case(DeriveOrderStatus::Cancelled, "cancelled")]
    #[case(DeriveOrderStatus::Expired, "expired")]
    fn test_order_status_wire_strings(#[case] variant: DeriveOrderStatus, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveOrderStatus::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveOrderCancelReason::Empty, "")]
    #[case(DeriveOrderCancelReason::UserRequest, "user_request")]
    #[case(DeriveOrderCancelReason::MmpTrigger, "mmp_trigger")]
    #[case(DeriveOrderCancelReason::InsufficientMargin, "insufficient_margin")]
    #[case(DeriveOrderCancelReason::SignedMaxFeeTooLow, "signed_max_fee_too_low")]
    #[case(DeriveOrderCancelReason::CancelOnDisconnect, "cancel_on_disconnect")]
    #[case(
        DeriveOrderCancelReason::IocOrMarketPartialFill,
        "ioc_or_market_partial_fill"
    )]
    #[case(
        DeriveOrderCancelReason::SessionKeyDeregistered,
        "session_key_deregistered"
    )]
    #[case(DeriveOrderCancelReason::SubaccountWithdrawn, "subaccount_withdrawn")]
    #[case(DeriveOrderCancelReason::Compliance, "compliance")]
    #[case(DeriveOrderCancelReason::ValidationFailed, "validation_failed")]
    #[case(
        DeriveOrderCancelReason::PostOnlyCrossMarket,
        "Post only order cannot cross the market"
    )]
    fn test_order_cancel_reason_wire_strings(
        #[case] variant: DeriveOrderCancelReason,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(
            serde_json::from_str::<DeriveOrderCancelReason>(&format!("\"{expected}\"")).unwrap(),
            variant
        );
    }

    #[rstest]
    #[case(DeriveRfqCancelReason::Empty, "")]
    #[case(DeriveRfqCancelReason::UserRequest, "user_request")]
    #[case(DeriveRfqCancelReason::InsufficientMargin, "insufficient_margin")]
    #[case(DeriveRfqCancelReason::SignedMaxFeeTooLow, "signed_max_fee_too_low")]
    #[case(DeriveRfqCancelReason::MmpTrigger, "mmp_trigger")]
    #[case(DeriveRfqCancelReason::CancelOnDisconnect, "cancel_on_disconnect")]
    #[case(
        DeriveRfqCancelReason::SessionKeyDeregistered,
        "session_key_deregistered"
    )]
    #[case(DeriveRfqCancelReason::SubaccountWithdrawn, "subaccount_withdrawn")]
    #[case(DeriveRfqCancelReason::RfqNoLongerOpen, "rfq_no_longer_open")]
    #[case(DeriveRfqCancelReason::Compliance, "compliance")]
    fn test_rfq_cancel_reason_wire_strings(
        #[case] variant: DeriveRfqCancelReason,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(
            serde_json::from_str::<DeriveRfqCancelReason>(&format!("\"{expected}\"")).unwrap(),
            variant
        );
    }

    #[rstest]
    fn test_order_cancel_reason_empty_string_round_trips() {
        let json = serde_json::to_string(&DeriveOrderCancelReason::Empty).unwrap();
        assert_eq!(json, "\"\"");
        let parsed: DeriveOrderCancelReason = serde_json::from_str("\"\"").unwrap();
        assert_eq!(parsed, DeriveOrderCancelReason::Empty);
    }

    #[rstest]
    #[case(DeriveLiquidityRole::Maker, "maker")]
    #[case(DeriveLiquidityRole::Taker, "taker")]
    fn test_liquidity_role_wire_strings(
        #[case] variant: DeriveLiquidityRole,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveLiquidityRole::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveTxStatus::Requested, "requested")]
    #[case(DeriveTxStatus::Pending, "pending")]
    #[case(DeriveTxStatus::Settled, "settled")]
    #[case(DeriveTxStatus::Reverted, "reverted")]
    #[case(DeriveTxStatus::Ignored, "ignored")]
    fn test_tx_status_wire_strings(#[case] variant: DeriveTxStatus, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveTxStatus::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveMarginType::Sm, "SM")]
    #[case(DeriveMarginType::Pm, "PM")]
    #[case(DeriveMarginType::Pm2, "PM2")]
    fn test_margin_type_wire_strings(#[case] variant: DeriveMarginType, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveMarginType::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveAuctionType::Solvent, "solvent")]
    #[case(DeriveAuctionType::Insolvent, "insolvent")]
    fn test_auction_type_wire_strings(#[case] variant: DeriveAuctionType, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveAuctionType::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveAuctionState::Ongoing, "ongoing")]
    #[case(DeriveAuctionState::Ended, "ended")]
    fn test_auction_state_wire_strings(
        #[case] variant: DeriveAuctionState,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveAuctionState::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveNotificationStatus::Unseen, "unseen")]
    #[case(DeriveNotificationStatus::Seen, "seen")]
    #[case(DeriveNotificationStatus::Hidden, "hidden")]
    fn test_notification_status_wire_strings(
        #[case] variant: DeriveNotificationStatus,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(
            DeriveNotificationStatus::from_str(expected).unwrap(),
            variant
        );
    }

    #[rstest]
    #[case(DeriveNotificationType::Deposit, "deposit")]
    #[case(DeriveNotificationType::Withdraw, "withdraw")]
    #[case(DeriveNotificationType::Transfer, "transfer")]
    #[case(DeriveNotificationType::Trade, "trade")]
    #[case(DeriveNotificationType::Settlement, "settlement")]
    #[case(DeriveNotificationType::Liquidation, "liquidation")]
    #[case(DeriveNotificationType::Types, "types")]
    fn test_notification_type_wire_strings(
        #[case] variant: DeriveNotificationType,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveNotificationType::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveBalanceUpdateType::Trade, "trade")]
    #[case(DeriveBalanceUpdateType::AssetDeposit, "asset_deposit")]
    #[case(DeriveBalanceUpdateType::AssetWithdrawal, "asset_withdrawal")]
    #[case(DeriveBalanceUpdateType::Transfer, "transfer")]
    #[case(DeriveBalanceUpdateType::SubaccountDeposit, "subaccount_deposit")]
    #[case(DeriveBalanceUpdateType::SubaccountWithdrawal, "subaccount_withdrawal")]
    #[case(DeriveBalanceUpdateType::Liquidation, "liquidation")]
    #[case(DeriveBalanceUpdateType::OnchainDriftFix, "onchain_drift_fix")]
    #[case(DeriveBalanceUpdateType::PerpSettlement, "perp_settlement")]
    #[case(DeriveBalanceUpdateType::OptionSettlement, "option_settlement")]
    #[case(DeriveBalanceUpdateType::InterestAccrual, "interest_accrual")]
    #[case(DeriveBalanceUpdateType::OnchainRevert, "onchain_revert")]
    #[case(DeriveBalanceUpdateType::DoubleRevert, "double_revert")]
    fn test_balance_update_type_wire_strings(
        #[case] variant: DeriveBalanceUpdateType,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(
            DeriveBalanceUpdateType::from_str(expected).unwrap(),
            variant
        );
    }

    #[rstest]
    #[case(DeriveComplianceStatus::Enabled, "enabled")]
    #[case(DeriveComplianceStatus::Disabled, "disabled")]
    fn test_compliance_status_wire_strings(
        #[case] variant: DeriveComplianceStatus,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveComplianceStatus::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveOrderbookDepth::D1, "1")]
    #[case(DeriveOrderbookDepth::D10, "10")]
    #[case(DeriveOrderbookDepth::D20, "20")]
    #[case(DeriveOrderbookDepth::D100, "100")]
    fn test_orderbook_depth_wire_strings(
        #[case] variant: DeriveOrderbookDepth,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveOrderbookDepth::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveOrderbookGroup::G1, "1")]
    #[case(DeriveOrderbookGroup::G10, "10")]
    #[case(DeriveOrderbookGroup::G100, "100")]
    fn test_orderbook_group_wire_strings(
        #[case] variant: DeriveOrderbookGroup,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveOrderbookGroup::from_str(expected).unwrap(), variant);
    }

    #[rstest]
    #[case(DeriveTickerInterval::Ms100, "100")]
    #[case(DeriveTickerInterval::Ms1000, "1000")]
    fn test_ticker_interval_wire_strings(
        #[case] variant: DeriveTickerInterval,
        #[case] expected: &str,
    ) {
        assert_eq!(variant.to_string(), expected);
        assert_eq!(DeriveTickerInterval::from_str(expected).unwrap(), variant);
    }
}
