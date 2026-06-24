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

//! Instrument definition diffing and emission for the Bybit adapter.

use ahash::{AHashMap, AHashSet};
use nautilus_common::messages::DataEvent;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

/// Returns `true` if the economically meaningful fields of two instruments differ.
///
/// [`InstrumentAny`]'s `PartialEq` compares only the instrument ID, and definitions carry
/// per-fetch timestamps that always change — so neither is usable for detecting "real" updates.
/// This compares the fields a strategy prices and sizes against, ignoring timestamps.
fn economics_differ(a: &InstrumentAny, b: &InstrumentAny) -> bool {
    a.maker_fee() != b.maker_fee()
        || a.taker_fee() != b.taker_fee()
        || a.price_increment() != b.price_increment()
        || a.size_increment() != b.size_increment()
        || a.min_quantity() != b.min_quantity()
        || a.min_notional() != b.min_notional()
}

/// Compares a fresh instrument snapshot against cached definitions, emitting [`DataEvent::Instrument`]
/// events for new and economically-changed instruments.
///
/// The cache is updated to reflect each meaningful change regardless of subscription, while emissions
/// are gated by `subscriptions`: only subscribed instruments produce events. Pass `None` to emit for
/// all changes (e.g. a venue-wide subscription).
pub fn diff_and_emit_instruments(
    new_instruments: &[InstrumentAny],
    cached: &mut AHashMap<InstrumentId, InstrumentAny>,
    subscriptions: Option<&AHashSet<InstrumentId>>,
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
) {
    let is_subscribed = |id: &InstrumentId| subscriptions.is_none_or(|subs| subs.contains(id));

    for instrument in new_instruments {
        let id = instrument.id();
        let changed = cached
            .get(&id)
            .is_none_or(|prev| economics_differ(prev, instrument));

        if changed {
            cached.insert(id, instrument.clone());
            if is_subscribed(&id)
                && let Err(e) = sender.send(DataEvent::Instrument(instrument.clone()))
            {
                log::error!("Failed to emit instrument event: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::{CryptoPerpetual, InstrumentAny},
        types::{Currency, Money, Price, Quantity},
    };
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;

    /// Builds a BTCUSDT linear perp with the given economic fields; everything else is fixed so a
    /// single varied field is what the diff sees.
    fn perp(
        maker_fee: Decimal,
        taker_fee: Decimal,
        size_increment: Quantity,
        min_notional: Option<Money>,
    ) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
            Symbol::from("BTCUSDT-LINEAR"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            1,     // price_precision
            3,     // size_precision
            Price::from("0.1"),
            size_increment,
            None,                          // multiplier
            None,                          // lot_size
            None,                          // max_quantity
            Some(Quantity::from("0.001")), // min_quantity
            None,                          // max_notional
            min_notional,
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            Some(maker_fee),
            Some(taker_fee),
            None,                 // tick_scheme
            None,                 // info
            UnixNanos::default(), // ts_event
            UnixNanos::default(), // ts_init
        ))
    }

    fn default_perp() -> InstrumentAny {
        perp(dec!(0.0001), dec!(0.00055), Quantity::from("0.001"), None)
    }

    #[test]
    fn test_emits_for_new_instrument() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let instrument = default_perp();
        let mut cached = AHashMap::new();

        diff_and_emit_instruments(std::slice::from_ref(&instrument), &mut cached, None, &tx);

        match rx.try_recv().expect("expected instrument event") {
            DataEvent::Instrument(emitted) => assert_eq!(emitted.id(), instrument.id()),
            _ => panic!("expected Instrument event"),
        }
        assert!(cached.contains_key(&instrument.id()));
    }

    #[test]
    fn test_no_emit_when_unchanged() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let instrument = default_perp();
        let mut cached = AHashMap::new();
        cached.insert(instrument.id(), default_perp());

        diff_and_emit_instruments(&[instrument], &mut cached, None, &tx);

        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_emits_on_fee_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = default_perp().id();
        let mut cached = AHashMap::new();
        cached.insert(id, default_perp());

        // Same instrument, higher taker fee.
        let updated = perp(dec!(0.0001), dec!(0.0008), Quantity::from("0.001"), None);
        diff_and_emit_instruments(&[updated], &mut cached, None, &tx);

        match rx
            .try_recv()
            .expect("expected instrument event on fee change")
        {
            DataEvent::Instrument(emitted) => assert_eq!(emitted.taker_fee(), dec!(0.0008)),
            _ => panic!("expected Instrument event"),
        }
        assert_eq!(cached.get(&id).unwrap().taker_fee(), dec!(0.0008));
    }

    #[test]
    fn test_emits_on_size_increment_and_min_notional_change() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = default_perp().id();

        // size_increment change
        let mut cached = AHashMap::new();
        cached.insert(id, default_perp());
        let bigger_step = perp(dec!(0.0001), dec!(0.00055), Quantity::from("0.002"), None);
        diff_and_emit_instruments(&[bigger_step], &mut cached, None, &tx);
        assert!(rx.try_recv().is_ok(), "size_increment change should emit");

        // min_notional change
        let mut cached = AHashMap::new();
        cached.insert(id, default_perp());
        let with_min = perp(
            dec!(0.0001),
            dec!(0.00055),
            Quantity::from("0.001"),
            Some(Money::new(5.0, Currency::USDT())),
        );
        diff_and_emit_instruments(&[with_min], &mut cached, None, &tx);
        assert!(rx.try_recv().is_ok(), "min_notional change should emit");
    }

    #[test]
    fn test_subscription_gating_updates_cache_but_only_emits_for_subscribed() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let id = default_perp().id();

        // Not subscribed: cache still updates, but no event.
        let empty_subs = AHashSet::new();
        let mut cached = AHashMap::new();
        cached.insert(id, default_perp());
        let updated = perp(dec!(0.0001), dec!(0.0008), Quantity::from("0.001"), None);

        diff_and_emit_instruments(&[updated], &mut cached, Some(&empty_subs), &tx);

        assert!(rx.try_recv().is_err(), "unsubscribed should not emit");
        assert_eq!(
            cached.get(&id).unwrap().taker_fee(),
            dec!(0.0008),
            "cache should update regardless of subscription"
        );

        // Subscribed: emits.
        let mut subs = AHashSet::new();
        subs.insert(id);
        let mut cached = AHashMap::new();
        cached.insert(id, default_perp());
        let updated = perp(dec!(0.0001), dec!(0.0008), Quantity::from("0.001"), None);

        diff_and_emit_instruments(&[updated], &mut cached, Some(&subs), &tx);

        assert!(rx.try_recv().is_ok(), "subscribed should emit");
    }
}
