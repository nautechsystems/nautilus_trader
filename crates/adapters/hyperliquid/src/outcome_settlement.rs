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

//! Runtime dispatch for HIP-4 outcome settlements.
//!
//! Each periodic `outcomeMeta` poll calls
//! [`crate::http::parse::derive_outcome_settlements`] to identify settled
//! (outcome_index, outcome_side) pairs, then materializes position-closing
//! [`FillReport`]s for any spot balance still holding a settled side token.
//! Settlements settle in USDH at 0 (losing) or 1 (winning), so each closing
//! fill carries a USDH commission of zero.
//!
//! State is tracked in [`OutcomeSettlementTracker`]; once a pair has been
//! dispatched it is not re-emitted on subsequent polls.

use ahash::AHashSet;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide},
    identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
    reports::FillReport,
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{converters::outcome_asset_id_to_instrument_id, types::HyperliquidAssetId},
    http::{
        models::SpotClearinghouseState,
        parse::{
            OUTCOME_PRICE_DECIMALS, OUTCOME_SIZE_DECIMALS, OutcomeSettlement, get_usdh_currency,
        },
    },
};

/// Tracks `(outcome_index, outcome_side)` pairs already dispatched so repeat
/// polls do not re-emit settlement fills.
#[derive(Debug, Default)]
pub struct OutcomeSettlementTracker {
    processed: AHashSet<(u32, u8)>,
}

impl OutcomeSettlementTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.processed.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.processed.is_empty()
    }

    #[must_use]
    pub fn contains(&self, outcome_index: u32, outcome_side: u8) -> bool {
        self.processed.contains(&(outcome_index, outcome_side))
    }

    fn mark(&mut self, outcome_index: u32, outcome_side: u8) -> bool {
        self.processed.insert((outcome_index, outcome_side))
    }
}

/// Materializes one closing [`FillReport`] per held outcome side token that
/// has newly settled.
///
/// `settlements` is the output of `derive_outcome_settlements`. Each
/// settlement is matched to a non-zero spot balance keyed by the `+E` token
/// name (Hyperliquid's spot-balance convention for outcome side tokens). Pairs
/// already in `tracker` are skipped; pairs that emit a fill are recorded.
///
/// Returns the synthetic fills ready to be forwarded through the execution
/// emitter. Returns an empty vector when no held outcome side token has
/// settled this round.
#[must_use]
pub fn build_settlement_fills(
    settlements: &[OutcomeSettlement],
    spot_state: &SpotClearinghouseState,
    tracker: &mut OutcomeSettlementTracker,
    account_id: AccountId,
    ts: UnixNanos,
) -> Vec<FillReport> {
    if settlements.is_empty() {
        return Vec::new();
    }

    let usdh = get_usdh_currency();
    let mut fills = Vec::new();

    for settlement in settlements {
        if tracker.contains(settlement.outcome_index, settlement.outcome_side) {
            continue;
        }

        let asset_id =
            HyperliquidAssetId::outcome(settlement.outcome_index, settlement.outcome_side);
        let Some(encoding) = asset_id.outcome_encoding() else {
            continue;
        };
        let token_coin = format!("+{encoding}");

        let Some(balance) = spot_state
            .balances
            .iter()
            .find(|b| b.coin.as_str() == token_coin && !b.total.is_zero())
        else {
            // No held position; mark processed so it does not re-trigger
            // on subsequent polls.
            tracker.mark(settlement.outcome_index, settlement.outcome_side);
            continue;
        };

        let instrument_id = match outcome_asset_id_to_instrument_id(asset_id) {
            Ok(id) => id,
            Err(e) => {
                log::error!("Outcome settlement skipped, instrument id resolution failed: {e}",);
                continue;
            }
        };

        if let Some(fill) = build_close_fill(
            instrument_id,
            account_id,
            settlement,
            balance.total,
            usdh,
            ts,
        ) {
            fills.push(fill);
            tracker.mark(settlement.outcome_index, settlement.outcome_side);
        }
    }

    fills
}

fn build_close_fill(
    instrument_id: InstrumentId,
    account_id: AccountId,
    settlement: &OutcomeSettlement,
    quantity: Decimal,
    currency: Currency,
    ts: UnixNanos,
) -> Option<FillReport> {
    let qty = Quantity::from_decimal_dp(quantity, OUTCOME_SIZE_DECIMALS as u8).ok()?;
    let price = Price::from_decimal_dp(
        Decimal::from(settlement.final_value),
        OUTCOME_PRICE_DECIMALS as u8,
    )
    .ok()?;

    // Deterministic identifiers so duplicate poll dispatch (e.g. process
    // restart with persisted tracker missing) does not yield distinct
    // trade ids for the same settlement.
    let tag = format!(
        "SETTLE-{}-{}",
        settlement.outcome_index, settlement.outcome_side,
    );
    let venue_order_id = VenueOrderId::new(format!("HYPERLIQUID-{tag}"));
    let trade_id = TradeId::new(format!("HYPERLIQUID-{tag}-{}", settlement.final_value));

    Some(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Sell,
        qty,
        price,
        Money::zero(currency),
        LiquiditySide::NoLiquiditySide,
        None,
        None,
        ts,
        ts,
        Some(UUID4::new()),
    ))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::http::models::SpotBalance;

    fn account() -> AccountId {
        AccountId::new("HYPERLIQUID-001")
    }

    fn spot_state_with(coin: &str, total: Decimal) -> SpotClearinghouseState {
        SpotClearinghouseState {
            balances: vec![SpotBalance {
                coin: Ustr::from(coin),
                token: None,
                total,
                hold: Decimal::ZERO,
                entry_ntl: None,
            }],
        }
    }

    #[rstest]
    fn empty_settlements_emit_nothing() {
        let mut tracker = OutcomeSettlementTracker::new();
        let state = SpotClearinghouseState::default();
        let fills =
            build_settlement_fills(&[], &state, &mut tracker, account(), UnixNanos::default());
        assert!(fills.is_empty());
        assert!(tracker.is_empty());
    }

    #[rstest]
    fn winning_side_emits_close_at_one_usdh() {
        let settlement = OutcomeSettlement {
            outcome_index: 1,
            outcome_side: 0,
            final_value: 1,
        };
        let state = spot_state_with("+10", dec!(25));
        let mut tracker = OutcomeSettlementTracker::new();

        let fills = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );

        assert_eq!(fills.len(), 1);
        let fill = &fills[0];
        assert_eq!(fill.instrument_id, InstrumentId::from("+10.HYPERLIQUID"));
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty.as_decimal(), dec!(25));
        // Quantity must match the outcome instrument's size precision (2),
        // not the USDH settlement currency precision (8)
        assert_eq!(fill.last_qty.precision, 2);
        assert_eq!(fill.last_px.as_decimal(), dec!(1));
        assert_eq!(fill.last_px.precision, 4);
        assert_eq!(fill.commission.currency.code.as_str(), "USDH");
        assert!(fill.commission.as_decimal().is_zero());
        assert!(tracker.contains(1, 0));
    }

    #[rstest]
    fn fractional_balance_rounds_to_outcome_size_precision() {
        // 25.1234567 should land at 25.12 once snapped to OUTCOME_SIZE_DECIMALS (2)
        let settlement = OutcomeSettlement {
            outcome_index: 4,
            outcome_side: 0,
            final_value: 1,
        };
        let state = spot_state_with("+40", dec!(25.1234567));
        let mut tracker = OutcomeSettlementTracker::new();

        let fills = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].last_qty.precision, 2);
        assert_eq!(fills[0].last_qty.as_decimal(), dec!(25.12));
    }

    #[rstest]
    fn losing_side_emits_close_at_zero_usdh() {
        let settlement = OutcomeSettlement {
            outcome_index: 1,
            outcome_side: 1,
            final_value: 0,
        };
        let state = spot_state_with("+11", dec!(10));
        let mut tracker = OutcomeSettlementTracker::new();

        let fills = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );

        assert_eq!(fills.len(), 1);
        let fill = &fills[0];
        assert_eq!(fill.instrument_id, InstrumentId::from("+11.HYPERLIQUID"));
        assert_eq!(fill.last_px.as_decimal(), dec!(0));
        assert!(tracker.contains(1, 1));
    }

    #[rstest]
    fn unheld_settlement_marks_tracker_without_fill() {
        let settlement = OutcomeSettlement {
            outcome_index: 5,
            outcome_side: 0,
            final_value: 1,
        };
        let state = SpotClearinghouseState::default();
        let mut tracker = OutcomeSettlementTracker::new();

        let fills = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );

        assert!(fills.is_empty());
        // Tracker still records the settlement so subsequent polls skip it
        assert!(tracker.contains(5, 0));
    }

    #[rstest]
    fn zero_balance_skipped_and_marked() {
        let settlement = OutcomeSettlement {
            outcome_index: 7,
            outcome_side: 0,
            final_value: 1,
        };
        let state = spot_state_with("+70", Decimal::ZERO);
        let mut tracker = OutcomeSettlementTracker::new();

        let fills = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );

        assert!(fills.is_empty());
        assert!(tracker.contains(7, 0));
    }

    #[rstest]
    fn repeated_settlement_is_idempotent() {
        let settlement = OutcomeSettlement {
            outcome_index: 2,
            outcome_side: 0,
            final_value: 1,
        };
        let state = spot_state_with("+20", dec!(5));
        let mut tracker = OutcomeSettlementTracker::new();

        let first = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );
        let second = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );

        assert_eq!(first.len(), 1);
        assert!(second.is_empty(), "repeat dispatch must not re-emit fills");
    }

    #[rstest]
    fn deterministic_identifiers_per_settlement() {
        let settlement = OutcomeSettlement {
            outcome_index: 3,
            outcome_side: 1,
            final_value: 0,
        };
        let state = spot_state_with("+31", dec!(1));
        let mut tracker = OutcomeSettlementTracker::new();
        let fills = build_settlement_fills(
            &[settlement],
            &state,
            &mut tracker,
            account(),
            UnixNanos::default(),
        );
        let fill = &fills[0];
        assert_eq!(fill.venue_order_id.as_str(), "HYPERLIQUID-SETTLE-3-1");
        assert_eq!(fill.trade_id.as_str(), "HYPERLIQUID-SETTLE-3-1-0");
    }
}
