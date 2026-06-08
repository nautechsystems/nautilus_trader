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

//! Typed JSON-RPC params for Derive private execution endpoints.

use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, B256, U256};
use anyhow::Context;
use nautilus_core::serialization::{
    deserialize_decimal, serialize_decimal_as_str, serialize_optional_decimal_as_str,
};
use nautilus_model::orders::{Order, OrderAny};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    common::{
        consts::DERIVE_NAUTILUS_REFERRAL_CODE,
        enums::{
            DeriveOrderSide, DeriveOrderType, DeriveTimeInForce, DeriveTriggerPriceType,
            DeriveTriggerType,
        },
        parse::{
            order_side_to_derive, order_type_to_derive, time_in_force_to_derive,
            trigger_order_type_to_derive, trigger_price_type_to_derive, trigger_type_to_derive,
        },
    },
    http::models::DeriveInstrument,
    signing::{
        eip712::{ActionContext, SignedAction},
        modules::{ModuleData, trade::TradeModuleData},
    },
};

/// Signed EIP-712 envelope shared by `private/order` and `private/replace`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveSignedEnvelope {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// Per-action nonce.
    pub nonce: u64,
    /// Session-key signer address.
    pub signer: String,
    /// Signature expiry in UNIX seconds.
    pub signature_expiry_sec: i64,
    /// 65-byte EIP-712 signature as `0x`-prefixed hex.
    pub signature: String,
}

impl DeriveSignedEnvelope {
    #[must_use]
    pub fn from_signed_action<M: ModuleData>(action: &SignedAction<'_, M>) -> Self {
        Self {
            subaccount_id: action.subaccount_id(),
            nonce: action.nonce(),
            signer: format!("{:?}", action.signer_address()),
            signature_expiry_sec: action.signature_expiry_sec(),
            signature: action.signature_hex(),
        }
    }
}

/// Params for `private/order`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveOrderParams {
    /// Signed action envelope.
    #[serde(flatten)]
    pub envelope: DeriveSignedEnvelope,
    /// Canonical Derive instrument name.
    pub instrument_name: Ustr,
    /// Order side.
    pub direction: DeriveOrderSide,
    /// Order type.
    pub order_type: DeriveOrderType,
    /// Time-in-force.
    pub time_in_force: DeriveTimeInForce,
    /// Signed limit price.
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal"
    )]
    pub limit_price: Decimal,
    /// Signed order amount.
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal"
    )]
    pub amount: Decimal,
    /// Signed per-contract fee cap.
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal"
    )]
    pub max_fee: Decimal,
    /// User label, mapped from Nautilus client order id.
    pub label: String,
    /// Nautilus referral code.
    pub referral_code: String,
    /// Reduce-only flag, omitted unless set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// MMP flag, omitted unless set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmp: Option<bool>,
    /// Trigger price for `private/trigger_order`; omitted for normal orders.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_decimal_as_str",
        deserialize_with = "nautilus_core::serialization::deserialize_optional_decimal"
    )]
    pub trigger_price: Option<Decimal>,
    /// Trigger price source for `private/trigger_order`; omitted for normal orders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_price_type: Option<DeriveTriggerPriceType>,
    /// Trigger side for `private/trigger_order`; omitted for normal orders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_type: Option<DeriveTriggerType>,
}

/// Params for `private/trigger_order`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveTriggerOrderParams {
    /// New signed trigger order body.
    #[serde(flatten)]
    pub order: DeriveOrderParams,
    /// WebSocket connection id supplied by the client.
    pub conn_id: String,
    /// Client-supplied Derive trigger order id.
    pub order_id: String,
}

/// Params for `private/replace`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveReplaceParams {
    /// New signed order body.
    #[serde(flatten)]
    pub order: DeriveOrderParams,
    /// Venue order id to atomically cancel.
    pub order_id_to_cancel: String,
}

/// Params for `private/cancel_trigger_order`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveCancelTriggerOrderParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// Venue order id.
    pub order_id: String,
}

impl DeriveCancelTriggerOrderParams {
    #[must_use]
    pub fn new(subaccount_id: u64, order_id: impl Into<String>) -> Self {
        Self {
            subaccount_id,
            order_id: order_id.into(),
        }
    }
}

/// Params for `private/cancel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveCancelParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// Canonical Derive instrument name.
    pub instrument_name: Ustr,
    /// Venue order id.
    pub order_id: String,
}

impl DeriveCancelParams {
    #[must_use]
    pub fn new(
        subaccount_id: u64,
        instrument_name: impl Into<Ustr>,
        order_id: impl Into<String>,
    ) -> Self {
        Self {
            subaccount_id,
            instrument_name: instrument_name.into(),
            order_id: order_id.into(),
        }
    }
}

/// Params for `private/cancel_all`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveCancelAllParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// Optional instrument scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_name: Option<Ustr>,
}

impl DeriveCancelAllParams {
    #[must_use]
    pub const fn new(subaccount_id: u64) -> Self {
        Self {
            subaccount_id,
            instrument_name: None,
        }
    }

    #[must_use]
    pub fn with_instrument_name(mut self, instrument_name: impl Into<Ustr>) -> Self {
        self.instrument_name = Some(instrument_name.into());
        self
    }
}

/// Params for `private/cancel_by_label`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveCancelByLabelParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// User label to cancel.
    pub label: String,
}

impl DeriveCancelByLabelParams {
    #[must_use]
    pub fn new(subaccount_id: u64, label: impl Into<String>) -> Self {
        Self {
            subaccount_id,
            label: label.into(),
        }
    }
}

/// Params for `private/get_subaccount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveGetSubaccountParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
}

impl DeriveGetSubaccountParams {
    #[must_use]
    pub const fn new(subaccount_id: u64) -> Self {
        Self { subaccount_id }
    }
}

/// Params for `private/get_open_orders`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveGetOpenOrdersParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
}

impl DeriveGetOpenOrdersParams {
    #[must_use]
    pub const fn new(subaccount_id: u64) -> Self {
        Self { subaccount_id }
    }
}

/// Params for `private/get_trigger_orders`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveGetTriggerOrdersParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
}

impl DeriveGetTriggerOrdersParams {
    #[must_use]
    pub const fn new(subaccount_id: u64) -> Self {
        Self { subaccount_id }
    }
}

/// Params for `private/get_order`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveGetOrderParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// Venue order id.
    pub order_id: String,
}

impl DeriveGetOrderParams {
    #[must_use]
    pub fn new(subaccount_id: u64, order_id: impl Into<String>) -> Self {
        Self {
            subaccount_id,
            order_id: order_id.into(),
        }
    }
}

/// Params for `private/get_order_history`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveGetOrderHistoryParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// Optional instrument scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_name: Option<Ustr>,
    /// Optional inclusive lower timestamp bound in UNIX milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_timestamp: Option<i64>,
    /// Optional inclusive upper timestamp bound in UNIX milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_timestamp: Option<i64>,
    /// 1-indexed page number.
    pub page: u32,
    /// Page size.
    pub page_size: u32,
}

impl DeriveGetOrderHistoryParams {
    #[must_use]
    pub fn new(subaccount_id: u64, page: u32, page_size: u32) -> Self {
        Self {
            subaccount_id,
            instrument_name: None,
            from_timestamp: None,
            to_timestamp: None,
            page,
            page_size,
        }
    }

    #[must_use]
    pub fn with_instrument_name(mut self, instrument_name: impl Into<Ustr>) -> Self {
        self.instrument_name = Some(instrument_name.into());
        self
    }

    #[must_use]
    pub const fn with_window(
        mut self,
        from_timestamp: Option<i64>,
        to_timestamp: Option<i64>,
    ) -> Self {
        self.from_timestamp = from_timestamp;
        self.to_timestamp = to_timestamp;
        self
    }
}

/// Params for `private/get_trade_history`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveGetTradeHistoryParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
    /// Optional instrument scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_name: Option<Ustr>,
    /// Optional inclusive lower timestamp bound in UNIX milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_timestamp: Option<i64>,
    /// Optional inclusive upper timestamp bound in UNIX milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_timestamp: Option<i64>,
    /// 1-indexed page number.
    pub page: u32,
    /// Page size.
    pub page_size: u32,
}

impl DeriveGetTradeHistoryParams {
    #[must_use]
    pub fn new(subaccount_id: u64, page: u32, page_size: u32) -> Self {
        Self {
            subaccount_id,
            instrument_name: None,
            from_timestamp: None,
            to_timestamp: None,
            page,
            page_size,
        }
    }

    #[must_use]
    pub fn with_instrument_name(mut self, instrument_name: impl Into<Ustr>) -> Self {
        self.instrument_name = Some(instrument_name.into());
        self
    }

    #[must_use]
    pub const fn with_window(
        mut self,
        from_timestamp: Option<i64>,
        to_timestamp: Option<i64>,
    ) -> Self {
        self.from_timestamp = from_timestamp;
        self.to_timestamp = to_timestamp;
        self
    }
}

/// Params for `private/get_positions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct DeriveGetPositionsParams {
    /// Owning subaccount identifier.
    pub subaccount_id: u64,
}

impl DeriveGetPositionsParams {
    #[must_use]
    pub const fn new(subaccount_id: u64) -> Self {
        Self { subaccount_id }
    }
}

/// Builds typed params for a signed `private/order` request.
///
/// `wallet` is the owner address, `signer_address` the session-key address,
/// and `nonce` / `signature_expiry_sec` come from
/// [`crate::signing::nonce::NonceManager`] and the configured expiry policy.
/// `explicit_price` overrides the limit price slot. Callers must supply it
/// for market orders because Derive signs the worst-acceptable price into the
/// EIP-712 trade module data.
///
/// # Errors
///
/// Returns an error when the order is not a Limit or Market order, when the
/// instrument's `base_asset_address` cannot be parsed, when decimal scaling
/// fails, when a Market order is submitted without an `explicit_price`, or
/// when EIP-712 signing fails.
#[expect(clippy::too_many_arguments)]
pub fn order_to_derive_payload(
    order: &OrderAny,
    instrument: &DeriveInstrument,
    subaccount_id: u64,
    wallet: Address,
    signer: &PrivateKeySigner,
    nonce: u64,
    signature_expiry_sec: i64,
    module_address: Address,
    domain_separator: B256,
    action_typehash: B256,
    max_fee: Decimal,
    explicit_price: Option<Decimal>,
) -> anyhow::Result<DeriveOrderParams> {
    validate_order_support(order)?;
    let limit_price = resolve_limit_price(order, explicit_price)?;
    let amount = order.quantity().as_decimal();
    let order_type = order_type_to_derive(order.order_type())?;
    let time_in_force = time_in_force_to_derive(order.time_in_force(), order.is_post_only())?;
    build_signed_order_params(
        order,
        instrument,
        subaccount_id,
        wallet,
        signer,
        nonce,
        signature_expiry_sec,
        module_address,
        domain_separator,
        action_typehash,
        max_fee,
        limit_price,
        amount,
        order_type,
        time_in_force,
        None,
    )
}

/// Builds typed params for a signed `private/trigger_order` request.
///
/// Derive stores trigger orders off-book until the venue trigger worker
/// submits the signed child order. `conn_id` and `order_id` are client-supplied
/// fields required by the WebSocket-only endpoint.
///
/// # Errors
///
/// Returns an error when the order is not one of StopMarket, StopLimit,
/// MarketIfTouched, or LimitIfTouched, when the trigger source is not
/// MarkPrice, when required prices are absent, or when EIP-712 signing fails.
#[expect(clippy::too_many_arguments)]
pub fn trigger_order_to_derive_payload(
    order: &OrderAny,
    instrument: &DeriveInstrument,
    subaccount_id: u64,
    wallet: Address,
    signer: &PrivateKeySigner,
    nonce: u64,
    signature_expiry_sec: i64,
    module_address: Address,
    domain_separator: B256,
    action_typehash: B256,
    max_fee: Decimal,
    explicit_price: Option<Decimal>,
    conn_id: impl Into<String>,
    order_id: impl Into<String>,
) -> anyhow::Result<DeriveTriggerOrderParams> {
    validate_trigger_order_support(order)?;
    let limit_price = resolve_limit_price(order, explicit_price)?;
    let amount = order.quantity().as_decimal();
    let order_type = trigger_order_type_to_derive(order.order_type())?;
    let time_in_force = time_in_force_to_derive(order.time_in_force(), order.is_post_only())?;
    let trigger_price = order.trigger_price().ok_or_else(|| {
        anyhow::anyhow!(
            "missing trigger price for Derive trigger order {}",
            order.client_order_id()
        )
    })?;
    let trigger_fields = DeriveTriggerFields {
        trigger_price: trigger_price.as_decimal(),
        trigger_price_type: trigger_price_type_to_derive(order.trigger_type())?,
        trigger_type: trigger_type_to_derive(order.order_type())?,
    };
    let order = build_signed_order_params(
        order,
        instrument,
        subaccount_id,
        wallet,
        signer,
        nonce,
        signature_expiry_sec,
        module_address,
        domain_separator,
        action_typehash,
        max_fee,
        limit_price,
        amount,
        order_type,
        time_in_force,
        Some(trigger_fields),
    )?;

    Ok(DeriveTriggerOrderParams {
        order,
        conn_id: conn_id.into(),
        order_id: order_id.into(),
    })
}

/// Builds typed params for a signed `private/replace` request.
///
/// Derive's replace endpoint atomically cancels a stale order and submits a
/// new signed order. The new-order half is signed against
/// [`TradeModuleData`] exactly like `private/order`.
///
/// # Errors
///
/// Returns an error when the order is not a Limit or Market order, when the
/// instrument's `base_asset_address` cannot be parsed, when decimal scaling
/// fails, when a Market order has no `explicit_price`, or when EIP-712 signing
/// fails.
#[expect(clippy::too_many_arguments)]
pub fn order_replace_to_derive_payload(
    order: &OrderAny,
    instrument: &DeriveInstrument,
    subaccount_id: u64,
    wallet: Address,
    signer: &PrivateKeySigner,
    nonce: u64,
    signature_expiry_sec: i64,
    module_address: Address,
    domain_separator: B256,
    action_typehash: B256,
    max_fee: Decimal,
    explicit_quantity: Option<Decimal>,
    explicit_price: Option<Decimal>,
    order_id_to_cancel: &str,
) -> anyhow::Result<DeriveReplaceParams> {
    validate_order_support(order)?;
    let limit_price = resolve_limit_price(order, explicit_price)?;
    let amount = explicit_quantity.unwrap_or_else(|| order.quantity().as_decimal());
    let order_type = order_type_to_derive(order.order_type())?;
    let time_in_force = time_in_force_to_derive(order.time_in_force(), order.is_post_only())?;
    let order = build_signed_order_params(
        order,
        instrument,
        subaccount_id,
        wallet,
        signer,
        nonce,
        signature_expiry_sec,
        module_address,
        domain_separator,
        action_typehash,
        max_fee,
        limit_price,
        amount,
        order_type,
        time_in_force,
        None,
    )?;

    Ok(DeriveReplaceParams {
        order,
        order_id_to_cancel: order_id_to_cancel.to_string(),
    })
}

fn validate_order_support(order: &OrderAny) -> anyhow::Result<()> {
    order_type_to_derive(order.order_type())?;
    time_in_force_to_derive(order.time_in_force(), order.is_post_only())?;
    Ok(())
}

fn validate_trigger_order_support(order: &OrderAny) -> anyhow::Result<()> {
    trigger_order_type_to_derive(order.order_type())?;
    time_in_force_to_derive(order.time_in_force(), order.is_post_only())?;
    trigger_price_type_to_derive(order.trigger_type())?;
    Ok(())
}

fn resolve_limit_price(
    order: &OrderAny,
    explicit_price: Option<Decimal>,
) -> anyhow::Result<Decimal> {
    match explicit_price {
        Some(p) => Ok(p),
        None => match order.price() {
            Some(p) => Ok(p.as_decimal()),
            None => anyhow::bail!(
                "missing limit price for order {} (market orders require an explicit slippage-adjusted price)",
                order.client_order_id(),
            ),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeriveTriggerFields {
    trigger_price: Decimal,
    trigger_price_type: DeriveTriggerPriceType,
    trigger_type: DeriveTriggerType,
}

#[expect(clippy::too_many_arguments)]
fn build_signed_order_params(
    order: &OrderAny,
    instrument: &DeriveInstrument,
    subaccount_id: u64,
    wallet: Address,
    signer: &PrivateKeySigner,
    nonce: u64,
    signature_expiry_sec: i64,
    module_address: Address,
    domain_separator: B256,
    action_typehash: B256,
    max_fee: Decimal,
    limit_price: Decimal,
    amount: Decimal,
    order_type: DeriveOrderType,
    time_in_force: DeriveTimeInForce,
    trigger_fields: Option<DeriveTriggerFields>,
) -> anyhow::Result<DeriveOrderParams> {
    let direction = order_side_to_derive(order.order_side())?;

    let asset_address: Address = instrument
        .base_asset_address
        .as_str()
        .parse()
        .with_context(|| {
            format!(
                "failed to parse base_asset_address `{}`",
                instrument.base_asset_address.as_str(),
            )
        })?;
    let sub_id =
        U256::from_str_radix(instrument.base_asset_sub_id.as_str(), 10).with_context(|| {
            format!(
                "failed to parse base_asset_sub_id `{}`",
                instrument.base_asset_sub_id.as_str(),
            )
        })?;

    let trade = TradeModuleData {
        asset_address,
        sub_id,
        limit_price,
        amount,
        max_fee,
        recipient_id: subaccount_id,
        is_bid: matches!(direction, DeriveOrderSide::Buy),
    };

    let ctx = ActionContext {
        subaccount_id,
        nonce,
        module_address,
        signature_expiry_sec,
        owner: wallet,
        signer: signer.address(),
    };

    let mut action = SignedAction::new(ctx, &trade, domain_separator, action_typehash);
    action
        .sign(signer)
        .context("failed to sign Derive trade action")?;

    Ok(DeriveOrderParams {
        envelope: DeriveSignedEnvelope::from_signed_action(&action),
        instrument_name: instrument.instrument_name,
        direction,
        order_type,
        time_in_force,
        limit_price,
        amount,
        max_fee,
        label: order.client_order_id().to_string(),
        referral_code: DERIVE_NAUTILUS_REFERRAL_CODE.to_string(),
        reduce_only: order.is_reduce_only().then_some(true),
        mmp: order.is_post_only().then_some(false),
        trigger_price: trigger_fields.map(|f| f.trigger_price),
        trigger_price_type: trigger_fields.map(|f| f.trigger_price_type),
        trigger_type: trigger_fields.map(|f| f.trigger_type),
    })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce, TriggerType},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId},
        orders::{LimitOrder, MarketOrder, OrderTestBuilder},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json::Value;

    use super::*;
    use crate::common::{consts::DERIVE_VENUE, enums::DeriveInstrumentType};

    fn canonical_wire<T: Serialize>(params: &T) -> String {
        let value = serde_json::to_value(params).unwrap();
        serde_json::to_string(&value).unwrap()
    }

    fn to_value<T: Serialize>(value: T) -> Value {
        serde_json::to_value(value).unwrap()
    }

    fn fixed_envelope(nonce: u64, signature: &str) -> DeriveSignedEnvelope {
        DeriveSignedEnvelope {
            subaccount_id: 30769,
            nonce,
            signer: "0xsigner".to_string(),
            signature_expiry_sec: 1_700_000_600 + (nonce as i64 - 123_456),
            signature: signature.to_string(),
        }
    }

    #[rstest]
    fn test_order_params_wire_round_trip_omits_unset_optionals() {
        let params = DeriveOrderParams {
            envelope: fixed_envelope(123_456, "0xabc"),
            instrument_name: "ETH-PERP".into(),
            direction: DeriveOrderSide::Buy,
            order_type: DeriveOrderType::Limit,
            time_in_force: DeriveTimeInForce::Gtc,
            limit_price: dec!(3500.01),
            amount: dec!(1.25),
            max_fee: dec!(0.5),
            label: "client-1".to_string(),
            referral_code: DERIVE_NAUTILUS_REFERRAL_CODE.to_string(),
            reduce_only: None,
            mmp: None,
            trigger_price: None,
            trigger_price_type: None,
            trigger_type: None,
        };

        let wire = canonical_wire(&params);
        let expected = include_str!("../../test_data/common/private_order_params_limit.json")
            .trim_end_matches('\n');
        let round_trip: DeriveOrderParams = serde_json::from_str(&wire).unwrap();

        assert_eq!(wire, expected);
        assert_eq!(round_trip, params);
    }

    #[rstest]
    fn test_replace_params_wire_round_trip_includes_set_optionals() {
        let params = DeriveReplaceParams {
            order: DeriveOrderParams {
                envelope: fixed_envelope(123_457, "0xdef"),
                instrument_name: "ETH-PERP".into(),
                direction: DeriveOrderSide::Sell,
                order_type: DeriveOrderType::Limit,
                time_in_force: DeriveTimeInForce::PostOnly,
                limit_price: dec!(3499.5),
                amount: dec!(2),
                max_fee: dec!(0),
                label: "client-2".to_string(),
                referral_code: DERIVE_NAUTILUS_REFERRAL_CODE.to_string(),
                reduce_only: Some(true),
                mmp: Some(false),
                trigger_price: None,
                trigger_price_type: None,
                trigger_type: None,
            },
            order_id_to_cancel: "ord-stale-1".to_string(),
        };

        let wire = canonical_wire(&params);
        let expected =
            include_str!("../../test_data/common/private_replace_params_reduce_mmp.json")
                .trim_end_matches('\n');
        let round_trip: DeriveReplaceParams = serde_json::from_str(&wire).unwrap();

        assert_eq!(wire, expected);
        assert_eq!(round_trip, params);
    }

    #[rstest]
    fn test_history_params_wire_round_trip_omits_unset_filters() {
        let params = DeriveGetOrderHistoryParams::new(30769, 2, 500);

        let wire = canonical_wire(&params);
        let expected =
            include_str!("../../test_data/common/private_order_history_params_required.json")
                .trim_end_matches('\n');
        let round_trip: DeriveGetOrderHistoryParams = serde_json::from_str(&wire).unwrap();

        assert_eq!(wire, expected);
        assert_eq!(round_trip, params);
    }

    #[rstest]
    fn test_trigger_order_params_wire_round_trip_includes_trigger_fields() {
        let params = DeriveTriggerOrderParams {
            order: DeriveOrderParams {
                envelope: fixed_envelope(123_458, "0xfeed"),
                instrument_name: "ETH-PERP".into(),
                direction: DeriveOrderSide::Sell,
                order_type: DeriveOrderType::Market,
                time_in_force: DeriveTimeInForce::Gtc,
                limit_price: dec!(3400),
                amount: dec!(0.1),
                max_fee: dec!(0.5),
                label: "client-stop-1".to_string(),
                referral_code: DERIVE_NAUTILUS_REFERRAL_CODE.to_string(),
                reduce_only: Some(true),
                mmp: None,
                trigger_price: Some(dec!(3450)),
                trigger_price_type: Some(DeriveTriggerPriceType::Mark),
                trigger_type: Some(DeriveTriggerType::Stoploss),
            },
            conn_id: "conn-1".to_string(),
            order_id: "trigger-order-1".to_string(),
        };

        let wire = canonical_wire(&params);
        let expected =
            include_str!("../../test_data/common/private_trigger_order_params_stop_market.json")
                .trim_end_matches('\n');
        let round_trip: DeriveTriggerOrderParams = serde_json::from_str(&wire).unwrap();

        assert_eq!(wire, expected);
        assert_eq!(round_trip, params);
    }

    fn sample_perp_instrument() -> DeriveInstrument {
        // Manually-constructed instrument record that satisfies signing's
        // address/sub-id parsing without depending on an on-disk fixture.
        DeriveInstrument {
            amount_step: dec!(0.001),
            base_asset_address: "0x000000000000000000000000000000000000abcd".into(),
            base_asset_sub_id: "42".into(),
            base_currency: "ETH".into(),
            base_fee: dec!(0),
            instrument_name: "ETH-PERP".into(),
            instrument_type: DeriveInstrumentType::Perp,
            is_active: true,
            maker_fee_rate: dec!(0.0001),
            mark_price_fee_rate_cap: None,
            maximum_amount: dec!(1000),
            minimum_amount: dec!(0.001),
            option_details: None,
            perp_details: None,
            quote_currency: "USDC".into(),
            scheduled_activation: 0,
            scheduled_deactivation: 0,
            taker_fee_rate: dec!(0.0005),
            tick_size: dec!(0.01),
        }
    }

    fn sample_signer() -> PrivateKeySigner {
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd"
            .parse()
            .unwrap()
    }

    fn sample_wallet() -> Address {
        "0x000000000000000000000000000000000000aaaa"
            .parse()
            .unwrap()
    }

    fn sample_module() -> Address {
        "0x000000000000000000000000000000000000bbbb"
            .parse()
            .unwrap()
    }

    fn sample_domain() -> B256 {
        "0x2222222222222222222222222222222222222222222222222222222222222222"
            .parse()
            .unwrap()
    }

    fn sample_typehash() -> B256 {
        "0x1111111111111111111111111111111111111111111111111111111111111111"
            .parse()
            .unwrap()
    }

    fn fresh_expiry_secs() -> i64 {
        (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)
            + 3600
    }

    fn build_test_limit_order(
        side: OrderSide,
        price: Decimal,
        qty: Decimal,
        post_only: bool,
        reduce_only: bool,
    ) -> OrderAny {
        build_test_limit_order_with_time_in_force(
            side,
            price,
            qty,
            TimeInForce::Gtc,
            post_only,
            reduce_only,
        )
    }

    fn build_test_limit_order_with_time_in_force(
        side: OrderSide,
        price: Decimal,
        qty: Decimal,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
    ) -> OrderAny {
        OrderAny::Limit(LimitOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-1"),
            InstrumentId::new(Symbol::new("ETH-PERP"), *DERIVE_VENUE),
            ClientOrderId::from("STRAT-PAYLOAD-1"),
            side,
            Quantity::from_decimal(qty).unwrap(),
            Price::from_decimal(price).unwrap(),
            time_in_force,
            None,
            post_only,
            reduce_only,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
    }

    fn build_test_stop_market_order() -> OrderAny {
        build_test_trigger_order(
            OrderType::StopMarket,
            OrderSide::Buy,
            None,
            TriggerType::Default,
        )
    }

    fn build_test_trigger_order(
        order_type: OrderType,
        side: OrderSide,
        price: Option<Decimal>,
        trigger_type: TriggerType,
    ) -> OrderAny {
        let mut builder = OrderTestBuilder::new(order_type);
        builder
            .instrument_id(InstrumentId::new(Symbol::new("ETH-PERP"), *DERIVE_VENUE))
            .client_order_id(ClientOrderId::from("STRAT-PAYLOAD-STOP"))
            .side(side)
            .quantity(Quantity::from_decimal(dec!(1)).unwrap())
            .trigger_price(Price::from_decimal(dec!(3600)).unwrap())
            .trigger_type(trigger_type)
            .time_in_force(TimeInForce::Gtc);

        if let Some(price) = price {
            builder.price(Price::from_decimal(price).unwrap());
        }

        builder.build()
    }

    fn build_test_market_order(side: OrderSide, qty: Decimal) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-1"),
            InstrumentId::new(Symbol::new("ETH-PERP"), *DERIVE_VENUE),
            ClientOrderId::from("STRAT-PAYLOAD-MK"),
            side,
            Quantity::from_decimal(qty).unwrap(),
            TimeInForce::Ioc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
    }

    #[rstest]
    fn test_order_to_derive_payload_limit_carries_all_required_fields() {
        let order = build_test_limit_order(OrderSide::Buy, dec!(3500), dec!(1), false, false);
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_001,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            None,
        )
        .map(to_value)
        .expect("payload built");

        assert_eq!(payload["instrument_name"], "ETH-PERP");
        assert_eq!(payload["direction"], "buy");
        assert_eq!(payload["order_type"], "limit");
        assert_eq!(payload["time_in_force"], "gtc");
        assert_eq!(payload["label"], "STRAT-PAYLOAD-1");
        assert_eq!(payload["referral_code"], "nautilus");
        assert_eq!(payload["limit_price"], "3500");
        assert_eq!(payload["amount"], "1");
        assert_eq!(payload["max_fee"], "1");
        assert_eq!(payload["subaccount_id"], 30769);
        assert_eq!(payload["nonce"], 17_000_000_000_001_u64);
        assert!(payload["signature_expiry_sec"].as_i64().unwrap() > 0);
        let signature = payload["signature"].as_str().unwrap();
        assert!(signature.starts_with("0x"));
        assert_eq!(signature.len(), 2 + 130, "65-byte sig = 132 hex chars");
        assert!(payload.get("reduce_only").is_none());
        assert!(payload.get("mmp").is_none());
    }

    #[rstest]
    fn test_order_to_derive_payload_accepts_uint256_sub_id() {
        let order = build_test_limit_order(OrderSide::Buy, dec!(1), dec!(0.1), true, false);
        let mut instrument = sample_perp_instrument();
        instrument.instrument_name = "ETH-20260529-2200-C".into();
        instrument.base_asset_sub_id = "39614082202024973918552016768".into();
        instrument.instrument_type = DeriveInstrumentType::Option;
        let signer = sample_signer();
        let payload = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_002,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            None,
        )
        .map(to_value)
        .expect("payload built");

        assert_eq!(payload["instrument_name"], "ETH-20260529-2200-C");
        assert_eq!(payload["time_in_force"], "post_only");
        assert!(payload["signature"].as_str().unwrap().starts_with("0x"));
    }

    #[rstest]
    #[case(TimeInForce::Gtc, false, "gtc")]
    #[case(TimeInForce::Ioc, false, "ioc")]
    #[case(TimeInForce::Fok, false, "fok")]
    #[case(TimeInForce::Gtc, true, "post_only")]
    fn test_order_to_derive_payload_carries_supported_time_in_force(
        #[case] time_in_force: TimeInForce,
        #[case] post_only: bool,
        #[case] expected: &str,
    ) {
        let order = build_test_limit_order_with_time_in_force(
            OrderSide::Buy,
            dec!(3500),
            dec!(1),
            time_in_force,
            post_only,
            false,
        );
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_002,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
        )
        .map(to_value)
        .expect("payload built");

        assert_eq!(payload["time_in_force"], expected);
    }

    #[rstest]
    fn test_order_to_derive_payload_emits_reduce_only_and_mmp_flags_when_set() {
        let order = build_test_limit_order(OrderSide::Sell, dec!(3500), dec!(1), true, true);
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_002,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
        )
        .map(to_value)
        .expect("payload built");

        assert_eq!(payload["direction"], "sell");
        assert_eq!(payload["time_in_force"], "post_only");
        assert_eq!(payload["reduce_only"], true);
        assert_eq!(payload["mmp"], false);
    }

    #[rstest]
    fn test_order_to_derive_payload_market_uses_explicit_price_override() {
        let order = build_test_market_order(OrderSide::Buy, dec!(0.5));
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_003,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            Some(dec!(3519)),
        )
        .map(to_value)
        .expect("payload built");

        assert_eq!(payload["order_type"], "market");
        assert_eq!(payload["limit_price"], "3519");
    }

    #[rstest]
    fn test_order_to_derive_payload_market_without_explicit_price_errors() {
        let order = build_test_market_order(OrderSide::Buy, dec!(0.5));
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let err = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_004,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
        )
        .expect_err("market without price must error");

        assert!(
            err.to_string().contains("missing limit price"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    #[case(TimeInForce::Day, false, "unsupported time in force")]
    #[case(TimeInForce::Day, true, "unsupported time in force")]
    #[case(TimeInForce::Ioc, true, "post-only Derive orders only support GTC")]
    #[case(TimeInForce::Fok, true, "post-only Derive orders only support GTC")]
    fn test_order_to_derive_payload_rejects_unsupported_tif(
        #[case] time_in_force: TimeInForce,
        #[case] post_only: bool,
        #[case] reason_fragment: &str,
    ) {
        let order = build_test_limit_order_with_time_in_force(
            OrderSide::Buy,
            dec!(3500),
            dec!(1),
            time_in_force,
            post_only,
            false,
        );
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let err = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_005,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
        )
        .expect_err("unsupported TIF must error");

        assert!(
            err.to_string().contains(reason_fragment),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    fn test_order_to_derive_payload_rejects_stop_order_before_price_resolution() {
        let order = build_test_stop_market_order();
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let err = order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_006,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
        )
        .expect_err("unsupported order type must error");

        assert!(
            err.to_string().contains("unsupported order type"),
            "unexpected error: {err}",
        );
        assert!(
            !err.to_string().contains("missing limit price"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    fn test_trigger_order_to_derive_payload_stop_market_uses_mark_trigger() {
        let order = build_test_trigger_order(
            OrderType::StopMarket,
            OrderSide::Sell,
            None,
            TriggerType::MarkPrice,
        );
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = trigger_order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_007,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            Some(dec!(3400)),
            "conn-1",
            "trigger-1",
        )
        .map(to_value)
        .expect("trigger payload built");

        assert_eq!(payload["conn_id"], "conn-1");
        assert_eq!(payload["order_id"], "trigger-1");
        assert_eq!(payload["direction"], "sell");
        assert_eq!(payload["order_type"], "market");
        assert_eq!(payload["limit_price"], "3400");
        assert_eq!(payload["trigger_price"], "3600");
        assert_eq!(payload["trigger_price_type"], "mark");
        assert_eq!(payload["trigger_type"], "stoploss");
    }

    #[rstest]
    fn test_trigger_order_to_derive_payload_stop_limit_maps_limit_stoploss() {
        let order = build_test_trigger_order(
            OrderType::StopLimit,
            OrderSide::Sell,
            Some(dec!(3500)),
            TriggerType::MarkPrice,
        );
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = trigger_order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_008,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            None,
            "conn-1",
            "trigger-2",
        )
        .map(to_value)
        .expect("trigger payload built");

        assert_eq!(payload["order_type"], "limit");
        assert_eq!(payload["limit_price"], "3500");
        assert_eq!(payload["trigger_type"], "stoploss");
    }

    #[rstest]
    #[case(
        OrderType::MarketIfTouched,
        DeriveOrderType::Market,
        DeriveTriggerType::Takeprofit
    )]
    #[case(
        OrderType::LimitIfTouched,
        DeriveOrderType::Limit,
        DeriveTriggerType::Takeprofit
    )]
    fn test_trigger_order_to_derive_payload_take_profit_types(
        #[case] order_type: OrderType,
        #[case] expected_order_type: DeriveOrderType,
        #[case] expected_trigger_type: DeriveTriggerType,
    ) {
        let price = if order_type == OrderType::LimitIfTouched {
            Some(dec!(3700))
        } else {
            None
        };
        let explicit_price = if price.is_none() {
            Some(dec!(3705))
        } else {
            None
        };
        let order =
            build_test_trigger_order(order_type, OrderSide::Buy, price, TriggerType::MarkPrice);
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = trigger_order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_009,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            explicit_price,
            "conn-1",
            "trigger-3",
        )
        .expect("trigger payload built");

        assert_eq!(payload.order.order_type, expected_order_type);
        assert_eq!(payload.order.trigger_type, Some(expected_trigger_type));
    }

    #[rstest]
    fn test_trigger_order_to_derive_payload_rejects_index_trigger_price_type() {
        let order = build_test_trigger_order(
            OrderType::StopMarket,
            OrderSide::Buy,
            None,
            TriggerType::IndexPrice,
        );
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let err = trigger_order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_010,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            Some(dec!(3618)),
            "conn-1",
            "trigger-4",
        )
        .expect_err("index trigger price type must fail");

        assert!(
            err.to_string()
                .contains("Derive currently accepts only MarkPrice"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    fn test_trigger_order_to_derive_payload_maps_default_trigger_type_to_mark() {
        let order = build_test_trigger_order(
            OrderType::StopMarket,
            OrderSide::Buy,
            None,
            TriggerType::Default,
        );
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = trigger_order_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_011,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            Some(dec!(3618)),
            "conn-1",
            "trigger-5",
        )
        .expect("default trigger type should map to mark");

        assert_eq!(
            payload.order.trigger_price_type,
            Some(DeriveTriggerPriceType::Mark),
        );
    }

    #[rstest]
    fn test_order_replace_to_derive_payload_stamps_cancel_clause_and_overrides() {
        let order = build_test_limit_order(OrderSide::Buy, dec!(3500), dec!(1), false, false);
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = order_replace_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_010,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(1),
            Some(dec!(2)),
            Some(dec!(3505)),
            "ord-stale-1",
        )
        .map(to_value)
        .expect("replace payload built");

        assert_eq!(payload["order_id_to_cancel"], "ord-stale-1");
        assert_eq!(payload["amount"], "2");
        assert_eq!(payload["limit_price"], "3505");
        assert_eq!(payload["direction"], "buy");
        assert_eq!(payload["order_type"], "limit");
        assert_eq!(payload["time_in_force"], "gtc");
        assert_eq!(payload["label"], "STRAT-PAYLOAD-1");
        assert_eq!(payload["subaccount_id"], 30769);
        assert_eq!(payload["nonce"], 17_000_000_000_010_u64);
        let signature = payload["signature"].as_str().unwrap();
        assert!(signature.starts_with("0x"));
        assert_eq!(signature.len(), 2 + 130);
    }

    #[rstest]
    fn test_order_replace_to_derive_payload_falls_back_to_cached_quantity_and_price() {
        let order = build_test_limit_order(OrderSide::Sell, dec!(3501), dec!(0.5), false, false);
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let payload = order_replace_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_011,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
            None,
            "ord-stale-2",
        )
        .map(to_value)
        .expect("replace payload built");

        assert_eq!(payload["order_id_to_cancel"], "ord-stale-2");
        assert_eq!(payload["amount"], "0.5");
        assert_eq!(payload["limit_price"], "3501");
        assert_eq!(payload["direction"], "sell");
    }

    #[rstest]
    fn test_order_replace_to_derive_payload_market_without_explicit_price_errors() {
        let order = build_test_market_order(OrderSide::Buy, dec!(0.5));
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let err = order_replace_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_012,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
            None,
            "ord-stale-3",
        )
        .expect_err("market replace without price must error");

        assert!(
            err.to_string().contains("missing limit price"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    fn test_order_replace_to_derive_payload_rejects_stop_order_before_price_resolution() {
        let order = build_test_stop_market_order();
        let instrument = sample_perp_instrument();
        let signer = sample_signer();
        let err = order_replace_to_derive_payload(
            &order,
            &instrument,
            30769,
            sample_wallet(),
            &signer,
            17_000_000_000_013,
            fresh_expiry_secs(),
            sample_module(),
            sample_domain(),
            sample_typehash(),
            dec!(0),
            None,
            None,
            "ord-stale-4",
        )
        .expect_err("unsupported order type must error");

        assert!(
            err.to_string().contains("unsupported order type"),
            "unexpected error: {err}",
        );
        assert!(
            !err.to_string().contains("missing limit price"),
            "unexpected error: {err}",
        );
    }
}
