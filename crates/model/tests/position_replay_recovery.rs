// -------------------------------------------------------------------------------------------------
// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// https://nautechsystems.io
// Licensed under the GNU Lesser General Public License Version 3.0.
// https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Recovery must preserve carried price restatements when more fills arrive.
use nautilus_core::UUID4;
use nautilus_model::{
    enums::OrderSide,
    events::{OrderFilled, order::spec::OrderFilledSpec},
    identifiers::{ClientOrderId, PositionId, TradeId},
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    position::{Position, PositionReplayEvent},
    types::{Price, Quantity},
};
use rstest::rstest;

fn fill(name: &str, side: OrderSide, price: &str, position_id: PositionId) -> OrderFilled {
    OrderFilledSpec::builder()
        .instrument_id(audusd_sim().id())
        .client_order_id(ClientOrderId::from(format!("O-{name}")))
        .trade_id(TradeId::from(format!("T-{name}")))
        .position_id(position_id)
        .order_side(side)
        .last_qty(Quantity::from(1))
        .last_px(Price::from(price))
        .build()
}

#[rstest]
#[case(false, false, false)]
#[case(false, true, false)]
#[case(true, false, false)]
#[case(true, true, false)]
#[case(false, false, true)]
#[case(false, true, true)]
#[case(true, false, true)]
#[case(true, true, true)]
fn carried_restatement_survives_recovery_and_further_fill(
    #[case] messagepack: bool,
    #[case] warm_after_reload: bool,
    #[case] virtual_id: bool,
) {
    let position_id = PositionId::from(if virtual_id { "P-RECOVERY" } else { "RECOVERY" });
    assert_eq!(position_id.is_virtual(), virtual_id);
    let fill = |name, side, price| fill(name, side, price, position_id);
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let opening = fill("OPEN", OrderSide::Buy, "1.00000");
    let mut position = Position::new(&instrument, opening.clone());
    // Warm the original-price index before the correction must update it.
    assert!(position.has_replay_trade_id(opening.trade_id));
    let mut correction = opening.clone();
    correction.event_id = UUID4::new();
    correction.last_px = Price::from("0.90000");
    position.try_apply(&correction).unwrap();
    assert_eq!(position.avg_px_open, Price::from("0.90000").as_decimal());
    assert!(position.replay_events.iter().any(|event| {
        matches!(event, PositionReplayEvent::Filled(fill) if fill.event_id == correction.event_id && fill.last_px == correction.last_px)
    }));
    position
        .try_apply(&fill("CLOSE", OrderSide::Sell, "1.10000"))
        .unwrap();
    position
        .try_apply(&fill("REOPEN", OrderSide::Buy, "1.20000"))
        .unwrap();
    // The corrected trade belongs to a completed cycle, not current-cycle events.
    assert_eq!(position.events.len(), 1);
    assert!(
        position
            .events
            .iter()
            .all(|event| event.trade_id != opening.trade_id)
    );
    assert!(position.requires_replay_state());
    assert!(position.has_replay_trade_id(opening.trade_id));

    let mut recovered: Position = if messagepack {
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&position).unwrap()).unwrap()
    } else {
        serde_json::from_slice(&serde_json::to_vec(&position).unwrap()).unwrap()
    };

    if warm_after_reload {
        assert!(recovered.requires_replay_state());
        assert!(recovered.has_replay_trade_id(opening.trade_id));
    }
    assert!(recovered.replay_events.iter().any(|event| {
        matches!(event, PositionReplayEvent::Filled(fill) if fill.event_id == correction.event_id && fill.last_px == correction.last_px)
    }));
    let further = fill("FURTHER", OrderSide::Buy, "1.40000");
    position.try_apply(&further).unwrap();
    recovered.try_apply(&further).unwrap();
    assert_eq!(recovered.quantity, Quantity::from(2));
    assert_eq!(recovered.avg_px_open, Price::from("1.30000").as_decimal());
    assert!(recovered.requires_replay_state());
    assert!(recovered.has_replay_trade_id(opening.trade_id));
    assert!(recovered.has_replay_trade_id(further.trade_id));
    assert_eq!(
        serde_json::to_value(&recovered).unwrap(),
        serde_json::to_value(&position).unwrap()
    );
}
