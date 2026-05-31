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

//! Strongly typed Lighter L2 transaction payloads.
//!
//! Each tx-info struct carries the fields the venue's sequencer reduces into
//! the signed message. The trait [`LighterTx`] expresses the two pieces of
//! per-tx-type information the encoder needs: the discriminant (`tx_type`)
//! and the body element sequence used as the Poseidon2 preimage.
//!
//! Field order in [`LighterTx::push_body_elements`] is locked: it matches
//! the `Hash(lighterChainId)` method on each `txtypes.L2*TxInfo` Go struct.
//! Any reordering breaks the byte-equality property the Layer 2 oracle
//! vectors enforce.

use crate::{common::enums::LighterTxType, signing::field::Fp};

/// Number of attribute slots reserved per L2 transaction.
///
/// Mirrors the upstream `NbAttributesPerTx` constant; the attribute hash is
/// padded out to this width so its preimage shape is the same regardless of
/// how many attributes are populated.
pub(super) const NB_ATTRIBUTES_PER_TX: usize = 4;

const ATTR_TYPE_INTEGRATOR_ACCOUNT_INDEX: u8 = 1;
const ATTR_TYPE_INTEGRATOR_TAKER_FEE: u8 = 2;
const ATTR_TYPE_INTEGRATOR_MAKER_FEE: u8 = 3;
const ATTR_TYPE_SKIP_NONCE: u8 = 4;

/// Per-transaction L2 attributes.
///
/// The fields map to the upstream `AttributeTypeInteg*` discriminants. A
/// transaction whose attributes are all zero emits no aggregated attribute
/// hash and the body hash is the signed hash directly; populating any field
/// switches the encoder onto the aggregation path defined alongside
/// [`super::sign_tx`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct L2TxAttributes {
    /// Integrator account index.
    pub integrator_account_index: u64,
    /// Integrator taker fee in fee-tick units.
    pub integrator_taker_fee: u32,
    /// Integrator maker fee in fee-tick units.
    pub integrator_maker_fee: u32,
    /// `1` to instruct the sequencer to skip nonce bookkeeping for this tx.
    pub skip_nonce: u8,
}

impl L2TxAttributes {
    /// Returns true when no attribute slot is populated. Encoders short-circuit
    /// to the body hash on empty attributes, matching the upstream `IsEmpty`
    /// contract on `txtypes.L2TxAttributes`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.integrator_account_index == 0
            && self.integrator_taker_fee == 0
            && self.integrator_maker_fee == 0
            && self.skip_nonce == 0
    }

    /// Returns the (type, value) pairs in the same ascending order
    /// `getNormalizedTypes` produces upstream, padded with zeros to
    /// [`NB_ATTRIBUTES_PER_TX`] entries.
    pub(super) fn normalized_pairs(&self) -> [(u8, u64); NB_ATTRIBUTES_PER_TX] {
        let mut filled = [(0u8, 0u64); NB_ATTRIBUTES_PER_TX];
        let mut i = 0;
        let mut push = |ty: u8, val: u64| {
            if val != 0 {
                filled[i] = (ty, val);
                i += 1;
            }
        };
        push(
            ATTR_TYPE_INTEGRATOR_ACCOUNT_INDEX,
            self.integrator_account_index,
        );
        push(
            ATTR_TYPE_INTEGRATOR_TAKER_FEE,
            u64::from(self.integrator_taker_fee),
        );
        push(
            ATTR_TYPE_INTEGRATOR_MAKER_FEE,
            u64::from(self.integrator_maker_fee),
        );
        push(ATTR_TYPE_SKIP_NONCE, u64::from(self.skip_nonce));
        filled
    }
}

/// Common per-order body fields shared by [`CreateOrderTxInfo`] and the
/// not-yet-implemented `CreateGroupedOrders` variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OrderInfo {
    /// Market identifier. Spot markets use `2048..=4094`, perps `0..=254`.
    pub market_index: i16,
    /// Caller-supplied unique-per-account order index.
    pub client_order_index: i64,
    /// Order size in base-asset ticks.
    pub base_amount: i64,
    /// Limit / trigger price in quote-asset ticks.
    pub price: u32,
    /// `true` for a sell, `false` for a buy. Wire-encoded as `1` or `0`.
    pub is_ask: bool,
    /// Order type discriminant; see `LighterOrderType` for the mapping.
    pub order_type: u8,
    /// Time-in-force discriminant; see `LighterTimeInForce` for the mapping.
    pub time_in_force: u8,
    /// `true` for a reduce-only order. Wire-encoded as `1` or `0`.
    pub reduce_only: bool,
    /// Trigger price for stop / take-profit variants; `0` when unused.
    pub trigger_price: u32,
    /// Order expiry in milliseconds; `0` for IOC, `-1` for the venue default.
    pub order_expiry: i64,
}

impl OrderInfo {
    fn append_body_elements(&self, elems: &mut Vec<Fp>) {
        elems.push(field_from_i16(self.market_index));
        elems.push(field_from_i64(self.client_order_index));
        elems.push(field_from_i64(self.base_amount));
        elems.push(field_from_u32(self.price));
        elems.push(field_from_u8(u8::from(self.is_ask)));
        elems.push(field_from_u8(self.order_type));
        elems.push(field_from_u8(self.time_in_force));
        elems.push(field_from_u8(u8::from(self.reduce_only)));
        elems.push(field_from_u32(self.trigger_price));
        elems.push(field_from_i64(self.order_expiry));
    }
}

/// Common context wrapping a tx body: account, key, nonce, expiry.
///
/// Lighter's hash preamble starts with `(chain_id, tx_type, nonce, expired_at)`
/// followed by the body fields. The wrapper avoids repeating the preamble in
/// every tx struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TxContext {
    /// L2 account index of the signer.
    pub account_index: i64,
    /// API key index registered for this account (`0..=254`).
    pub api_key_index: u8,
    /// Lighter L2 sequence number. Reset only via the sequencer's reset rules.
    pub nonce: i64,
    /// Tx expiry in milliseconds; rejected if the sequencer sees it past wall.
    pub expired_at: i64,
}

/// Marker trait every signed L2 transaction implements.
///
/// The hash preimage is `[chain_id, tx_type, nonce, expired_at, account_index,
/// api_key_index, ...body]`. Implementations are responsible for appending the
/// per-type body elements after the preamble.
pub trait LighterTx {
    /// L2 transaction type discriminant emitted on the wire.
    fn tx_type(&self) -> LighterTxType;

    /// Common preamble fields.
    fn context(&self) -> TxContext;

    /// Per-transaction attributes; defaults to an empty set.
    fn attributes(&self) -> L2TxAttributes {
        L2TxAttributes::default()
    }

    /// Append the body field elements after the preamble. Caller pushes the
    /// preamble; implementations push the body in the upstream-fixed order.
    fn push_body_elements(&self, elems: &mut Vec<Fp>);

    /// Build the full hash preimage `[preamble || body]` ready to feed to
    /// `hash_to_quintic_extension`.
    fn hash_elements(&self, chain_id: u32) -> Vec<Fp> {
        let ctx = self.context();
        let mut elems = Vec::with_capacity(16);
        elems.push(field_from_u32(chain_id));
        elems.push(field_from_u8(self.tx_type() as u8));
        elems.push(field_from_i64(ctx.nonce));
        elems.push(field_from_i64(ctx.expired_at));
        elems.push(field_from_i64(ctx.account_index));
        elems.push(field_from_u8(ctx.api_key_index));
        self.push_body_elements(&mut elems);
        elems
    }
}

/// `CreateOrder` (`tx_type = 14`) — submits a new order to the sequencer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CreateOrderTxInfo {
    /// Common preamble fields.
    pub context: TxContext,
    /// Order body.
    pub order: OrderInfo,
    /// L2 attributes.
    pub attributes: L2TxAttributes,
}

impl LighterTx for CreateOrderTxInfo {
    fn tx_type(&self) -> LighterTxType {
        LighterTxType::CreateOrder
    }

    fn context(&self) -> TxContext {
        self.context
    }

    fn attributes(&self) -> L2TxAttributes {
        self.attributes
    }

    fn push_body_elements(&self, elems: &mut Vec<Fp>) {
        self.order.append_body_elements(elems);
    }
}

/// `CancelOrder` (`tx_type = 15`): cancels a single live order by index.
///
/// `CancelOrder` only accepts the `skip_nonce` L2 attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CancelOrderTxInfo {
    /// Common preamble fields.
    pub context: TxContext,
    /// Market the order lives on.
    pub market_index: i16,
    /// Either the venue's order index or the caller's client order index.
    pub index: i64,
    /// `1` to instruct the sequencer to skip nonce bookkeeping for this tx.
    pub skip_nonce: u8,
}

impl LighterTx for CancelOrderTxInfo {
    fn tx_type(&self) -> LighterTxType {
        LighterTxType::CancelOrder
    }

    fn context(&self) -> TxContext {
        self.context
    }

    fn attributes(&self) -> L2TxAttributes {
        L2TxAttributes {
            skip_nonce: self.skip_nonce,
            ..Default::default()
        }
    }

    fn push_body_elements(&self, elems: &mut Vec<Fp>) {
        elems.push(field_from_i16(self.market_index));
        elems.push(field_from_i64(self.index));
    }
}

/// `ModifyOrder` (`tx_type = 17`) — amends size / price on a live order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModifyOrderTxInfo {
    /// Common preamble fields.
    pub context: TxContext,
    /// Market the order lives on.
    pub market_index: i16,
    /// Either the venue's order index or the caller's client order index.
    pub index: i64,
    /// Replacement size in base-asset ticks.
    pub base_amount: i64,
    /// Replacement limit price in quote-asset ticks.
    pub price: u32,
    /// Replacement trigger price (`0` to clear).
    pub trigger_price: u32,
    /// L2 attributes.
    pub attributes: L2TxAttributes,
}

impl LighterTx for ModifyOrderTxInfo {
    fn tx_type(&self) -> LighterTxType {
        LighterTxType::ModifyOrder
    }

    fn context(&self) -> TxContext {
        self.context
    }

    fn attributes(&self) -> L2TxAttributes {
        self.attributes
    }

    fn push_body_elements(&self, elems: &mut Vec<Fp>) {
        elems.push(field_from_i16(self.market_index));
        elems.push(field_from_i64(self.index));
        elems.push(field_from_i64(self.base_amount));
        elems.push(field_from_u32(self.price));
        elems.push(field_from_u32(self.trigger_price));
    }
}

/// `ApproveIntegrator` (`tx_type = 45`): approves an integrator account index.
///
/// The upstream FFI wrapper for this tx kind passes only the `skip_nonce` L2
/// attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ApproveIntegratorTxInfo {
    /// Common preamble fields.
    pub context: TxContext,
    /// Integrator account index to approve.
    pub integrator_account_index: i64,
    /// Maximum perps taker fee the integrator may charge.
    pub max_perps_taker_fee: u32,
    /// Maximum perps maker fee the integrator may charge.
    pub max_perps_maker_fee: u32,
    /// Maximum spot taker fee the integrator may charge.
    pub max_spot_taker_fee: u32,
    /// Maximum spot maker fee the integrator may charge.
    pub max_spot_maker_fee: u32,
    /// Approval expiry in milliseconds; `0` revokes.
    pub approval_expiry: i64,
    /// `1` to instruct the sequencer to skip nonce bookkeeping for this tx.
    pub skip_nonce: u8,
}

impl LighterTx for ApproveIntegratorTxInfo {
    fn tx_type(&self) -> LighterTxType {
        LighterTxType::ApproveIntegrator
    }

    fn context(&self) -> TxContext {
        self.context
    }

    fn attributes(&self) -> L2TxAttributes {
        L2TxAttributes {
            skip_nonce: self.skip_nonce,
            ..Default::default()
        }
    }

    fn push_body_elements(&self, elems: &mut Vec<Fp>) {
        elems.push(field_from_i64(self.integrator_account_index));
        elems.push(field_from_u32(self.max_perps_taker_fee));
        elems.push(field_from_u32(self.max_perps_maker_fee));
        elems.push(field_from_u32(self.max_spot_taker_fee));
        elems.push(field_from_u32(self.max_spot_maker_fee));
        elems.push(field_from_i64(self.approval_expiry));
    }
}

/// `CancelAllOrders` (`tx_type = 16`): account-wide cancel directive.
///
/// Body fields mirror the `SignCancelAllOrders(int cTimeInForce, long long
/// int cTime, ...)` FFI signature in the upstream cgo header. `time_in_force`
/// is one of [`crate::common::enums::LighterCancelAllTimeInForce`] (Immediate,
/// Scheduled, AbortScheduled); `scheduled_time_ms` is the wall-clock instant
/// the sequencer should fire the cancel at, in milliseconds (ignored for
/// `Immediate`).
///
/// Like [`CancelOrderTxInfo`], the FFI wrapper for this kind passes only
/// `skip_nonce`, so only that attribute is carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CancelAllOrdersTxInfo {
    /// Common preamble fields.
    pub context: TxContext,
    /// Cancel scheduling discriminant (`Immediate`, `Scheduled`, `AbortScheduled`).
    pub time_in_force: u8,
    /// Scheduled fire time in milliseconds. `0` for `Immediate`.
    pub scheduled_time_ms: i64,
    /// `1` to instruct the sequencer to skip nonce bookkeeping for this tx.
    pub skip_nonce: u8,
}

impl LighterTx for CancelAllOrdersTxInfo {
    fn tx_type(&self) -> LighterTxType {
        LighterTxType::CancelAllOrders
    }

    fn context(&self) -> TxContext {
        self.context
    }

    fn attributes(&self) -> L2TxAttributes {
        L2TxAttributes {
            skip_nonce: self.skip_nonce,
            ..Default::default()
        }
    }

    fn push_body_elements(&self, elems: &mut Vec<Fp>) {
        elems.push(field_from_u8(self.time_in_force));
        elems.push(field_from_i64(self.scheduled_time_ms));
    }
}

/// `UpdateLeverage` (`tx_type = 20`): change leverage / margin mode for a market.
///
/// Body fields mirror the upstream `txtypes.L2UpdateLeverageTxInfo` Go
/// struct: `MarketIndex int16`, `InitialMarginFraction uint16`,
/// `MarginMode uint8`. `initial_margin_fraction` is in 1e-4 ticks
/// (`500` = 5% initial margin = 20x leverage), capped at the venue's
/// `MarginFractionTick = 10_000`. `margin_mode` is
/// [`crate::common::enums::LighterPositionMarginMode`] (`Cross`, `Isolated`).
///
/// Like [`CancelAllOrdersTxInfo`], only `skip_nonce` is carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UpdateLeverageTxInfo {
    /// Common preamble fields.
    pub context: TxContext,
    /// Market the change applies to.
    pub market_index: i16,
    /// Initial margin fraction in 1e-4 ticks; `500` = 5% = 20x leverage.
    pub initial_margin_fraction: u16,
    /// Margin-mode discriminant (`Cross`, `Isolated`).
    pub margin_mode: u8,
    /// `1` to instruct the sequencer to skip nonce bookkeeping for this tx.
    pub skip_nonce: u8,
}

impl LighterTx for UpdateLeverageTxInfo {
    fn tx_type(&self) -> LighterTxType {
        LighterTxType::UpdateLeverage
    }

    fn context(&self) -> TxContext {
        self.context
    }

    fn attributes(&self) -> L2TxAttributes {
        L2TxAttributes {
            skip_nonce: self.skip_nonce,
            ..Default::default()
        }
    }

    fn push_body_elements(&self, elems: &mut Vec<Fp>) {
        elems.push(field_from_i16(self.market_index));
        elems.push(field_from_u16(self.initial_margin_fraction));
        elems.push(field_from_u8(self.margin_mode));
    }
}

#[inline]
fn field_from_u8(v: u8) -> Fp {
    Fp::from_u64_reduce(u64::from(v))
}

#[inline]
fn field_from_u16(v: u16) -> Fp {
    Fp::from_u64_reduce(u64::from(v))
}

#[inline]
fn field_from_u32(v: u32) -> Fp {
    Fp::from_u64_reduce(u64::from(v))
}

#[inline]
fn field_from_i16(v: i16) -> Fp {
    field_from_i64(i64::from(v))
}

// Negative values reach the field via Go's `GoldilocksField(int64)` cast,
// which reinterprets the bit pattern as `uint64`; reproduce that here so the
// preimage stays byte-equal with what the upstream signer hashes for any
// negative tx-info field (e.g. nil-flag sentinels like `OrderExpiry = -1`).
#[inline]
fn field_from_i64(v: i64) -> Fp {
    Fp::from_u64_reduce(v as u64)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn ctx() -> TxContext {
        TxContext {
            account_index: 12_345,
            api_key_index: 5,
            nonce: 7,
            expired_at: 1_777_804_395_089,
        }
    }

    #[rstest]
    fn create_order_preamble_then_body() {
        let tx = CreateOrderTxInfo {
            context: ctx(),
            order: OrderInfo {
                market_index: 0,
                client_order_index: 123,
                base_amount: 1_000,
                price: 405_000,
                is_ask: true,
                order_type: 0,
                time_in_force: 1,
                reduce_only: false,
                trigger_price: 0,
                order_expiry: 1_735_689_600_000,
            },
            attributes: L2TxAttributes::default(),
        };
        let elems = tx.hash_elements(300);

        // The full 16-element preimage is asserted explicitly so a body-order
        // mutation fails this test independently of the oracle hash equality.
        let expected = [
            field_from_u32(300),               // chain_id
            field_from_u8(14),                 // tx_type
            field_from_i64(7),                 // nonce
            field_from_i64(1_777_804_395_089), // expired_at
            field_from_i64(12_345),            // account_index
            field_from_u8(5),                  // api_key_index
            field_from_i16(0),                 // market_index
            field_from_i64(123),               // client_order_index
            field_from_i64(1_000),             // base_amount
            field_from_u32(405_000),           // price
            field_from_u8(1),                  // is_ask (true -> 1)
            field_from_u8(0),                  // order_type
            field_from_u8(1),                  // time_in_force
            field_from_u8(0),                  // reduce_only (false -> 0)
            field_from_u32(0),                 // trigger_price
            field_from_i64(1_735_689_600_000), // order_expiry
        ];
        assert_eq!(elems.as_slice(), expected.as_slice());
    }

    #[rstest]
    fn cancel_order_preamble_then_body() {
        let tx = CancelOrderTxInfo {
            context: ctx(),
            market_index: 0,
            index: 123,
            skip_nonce: 0,
        };
        let elems = tx.hash_elements(300);

        let expected = [
            field_from_u32(300),               // chain_id
            field_from_u8(15),                 // tx_type (CancelOrder)
            field_from_i64(7),                 // nonce
            field_from_i64(1_777_804_395_089), // expired_at
            field_from_i64(12_345),            // account_index
            field_from_u8(5),                  // api_key_index
            field_from_i16(0),                 // market_index
            field_from_i64(123),               // index
        ];
        assert_eq!(elems.as_slice(), expected.as_slice());
    }

    #[rstest]
    fn modify_order_preamble_then_body() {
        let tx = ModifyOrderTxInfo {
            context: ctx(),
            market_index: 0,
            index: 123,
            base_amount: 1_100,
            price: 410_000,
            trigger_price: 0,
            attributes: L2TxAttributes::default(),
        };
        let elems = tx.hash_elements(300);

        let expected = [
            field_from_u32(300),               // chain_id
            field_from_u8(17),                 // tx_type (ModifyOrder)
            field_from_i64(7),                 // nonce
            field_from_i64(1_777_804_395_089), // expired_at
            field_from_i64(12_345),            // account_index
            field_from_u8(5),                  // api_key_index
            field_from_i16(0),                 // market_index
            field_from_i64(123),               // index
            field_from_i64(1_100),             // base_amount
            field_from_u32(410_000),           // price
            field_from_u32(0),                 // trigger_price
        ];
        assert_eq!(elems.as_slice(), expected.as_slice());
    }

    #[rstest]
    fn approve_integrator_preamble_then_body() {
        let tx = ApproveIntegratorTxInfo {
            context: ctx(),
            integrator_account_index: 723_813,
            max_perps_taker_fee: 500,
            max_perps_maker_fee: 200,
            max_spot_taker_fee: 600,
            max_spot_maker_fee: 300,
            approval_expiry: 1_780_000_000_000,
            skip_nonce: 0,
        };
        let elems = tx.hash_elements(300);

        let expected = [
            field_from_u32(300),               // chain_id
            field_from_u8(45),                 // tx_type (ApproveIntegrator)
            field_from_i64(7),                 // nonce
            field_from_i64(1_777_804_395_089), // expired_at
            field_from_i64(12_345),            // account_index
            field_from_u8(5),                  // api_key_index
            field_from_i64(723_813),           // integrator_account_index
            field_from_u32(500),               // max_perps_taker_fee
            field_from_u32(200),               // max_perps_maker_fee
            field_from_u32(600),               // max_spot_taker_fee
            field_from_u32(300),               // max_spot_maker_fee
            field_from_i64(1_780_000_000_000), // approval_expiry
        ];
        assert_eq!(elems.as_slice(), expected.as_slice());
    }

    #[rstest]
    #[case::immediate(0, 0)]
    #[case::scheduled(1, 1_800_000_000_000)]
    #[case::abort_scheduled(2, 0)]
    fn cancel_all_orders_preamble_then_body(
        #[case] time_in_force: u8,
        #[case] scheduled_time_ms: i64,
    ) {
        let tx = CancelAllOrdersTxInfo {
            context: ctx(),
            time_in_force,
            scheduled_time_ms,
            skip_nonce: 0,
        };
        let elems = tx.hash_elements(300);

        let expected = [
            field_from_u32(300),               // chain_id
            field_from_u8(16),                 // tx_type (CancelAllOrders)
            field_from_i64(7),                 // nonce
            field_from_i64(1_777_804_395_089), // expired_at
            field_from_i64(12_345),            // account_index
            field_from_u8(5),                  // api_key_index
            field_from_u8(time_in_force),      // time_in_force
            field_from_i64(scheduled_time_ms), // scheduled_time_ms
        ];
        assert_eq!(elems.as_slice(), expected.as_slice());
    }

    #[rstest]
    #[case::cross(0)]
    #[case::isolated(1)]
    fn update_leverage_preamble_then_body(#[case] margin_mode: u8) {
        let tx = UpdateLeverageTxInfo {
            context: ctx(),
            market_index: 3,
            initial_margin_fraction: 500, // 5% = 20x
            margin_mode,
            skip_nonce: 0,
        };
        let elems = tx.hash_elements(300);

        let expected = [
            field_from_u32(300),               // chain_id
            field_from_u8(20),                 // tx_type (UpdateLeverage)
            field_from_i64(7),                 // nonce
            field_from_i64(1_777_804_395_089), // expired_at
            field_from_i64(12_345),            // account_index
            field_from_u8(5),                  // api_key_index
            field_from_i16(3),                 // market_index
            field_from_u16(500),               // initial_margin_fraction
            field_from_u8(margin_mode),        // margin_mode
        ];
        assert_eq!(elems.as_slice(), expected.as_slice());
    }

    #[rstest]
    #[case::default(L2TxAttributes::default(), true)]
    #[case::integrator_account_only(
        L2TxAttributes { integrator_account_index: 1, ..Default::default() },
        false,
    )]
    #[case::taker_fee_only(
        L2TxAttributes { integrator_taker_fee: 1, ..Default::default() },
        false,
    )]
    #[case::maker_fee_only(
        L2TxAttributes { integrator_maker_fee: 1, ..Default::default() },
        false,
    )]
    #[case::skip_nonce_only(
        L2TxAttributes { skip_nonce: 1, ..Default::default() },
        false,
    )]
    fn attributes_is_empty_truth_table(#[case] attrs: L2TxAttributes, #[case] expected: bool) {
        assert_eq!(attrs.is_empty(), expected);
    }

    #[rstest]
    #[case::all_empty(L2TxAttributes::default(), [(0, 0); NB_ATTRIBUTES_PER_TX])]
    #[case::skip_only(
        L2TxAttributes { skip_nonce: 1, ..Default::default() },
        [(4, 1), (0, 0), (0, 0), (0, 0)],
    )]
    #[case::all_set(
        L2TxAttributes {
            integrator_account_index: 100,
            integrator_taker_fee: 50,
            integrator_maker_fee: 20,
            skip_nonce: 1,
        },
        [(1, 100), (2, 50), (3, 20), (4, 1)],
    )]
    #[case::sparse_with_padding(
        L2TxAttributes {
            integrator_account_index: 723_813,
            integrator_taker_fee: 0,
            integrator_maker_fee: 100,
            skip_nonce: 0,
        },
        [(1, 723_813), (3, 100), (0, 0), (0, 0)],
    )]
    #[case::account_and_skip_only(
        L2TxAttributes {
            integrator_account_index: 7,
            integrator_maker_fee: 9,
            ..Default::default()
        },
        [(1, 7), (3, 9), (0, 0), (0, 0)],
    )]
    fn normalized_pairs_truth_table(
        #[case] attrs: L2TxAttributes,
        #[case] expected: [(u8, u64); NB_ATTRIBUTES_PER_TX],
    ) {
        assert_eq!(attrs.normalized_pairs(), expected);
    }

    #[rstest]
    fn negative_int_field_matches_go_cast() {
        // Go's `GoldilocksField(int64(-1))` reinterprets bits to `u64::MAX`;
        // our constructor goes through the same cast, then reduces.
        let from_neg_one = field_from_i64(-1);
        let from_u64_max = Fp::from_u64_reduce(u64::MAX);
        assert_eq!(from_neg_one, from_u64_max);
    }

    proptest! {
        /// `field_from_i64` matches the bit-cast `as u64` lift through
        /// `Fp::from_u64_reduce` for any signed input.
        #[rstest]
        fn prop_field_from_i64_matches_u64_cast(v in any::<i64>()) {
            prop_assert_eq!(field_from_i64(v), Fp::from_u64_reduce(v as u64));
        }

        /// `field_from_i16` round-trips through the same cast.
        #[rstest]
        fn prop_field_from_i16_matches_u64_cast(v in any::<i16>()) {
            prop_assert_eq!(field_from_i16(v), Fp::from_u64_reduce(i64::from(v) as u64));
        }
    }
}
