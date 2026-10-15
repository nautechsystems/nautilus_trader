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

use super::{DataEngine, InstrumentId, Quantity, QuoteTick, SyntheticInstrument, TradeTick};

impl DataEngine {
    /// Returns all synthetic instrument IDs for which quote subscriptions exist.
    #[must_use]
    pub fn subscribed_synthetic_quotes(&self) -> Vec<InstrumentId> {
        self.subscribed_synthetic_quotes.keys().copied().collect()
    }

    /// Returns all synthetic instrument IDs for which trade subscriptions exist.
    #[must_use]
    pub fn subscribed_synthetic_trades(&self) -> Vec<InstrumentId> {
        self.subscribed_synthetic_trades.keys().copied().collect()
    }

    pub(super) fn synthetic_quotes_from_quote(&self, update: QuoteTick) -> Vec<QuoteTick> {
        let Some(synthetics) = self.synthetic_quote_feeds.get(&update.instrument_id) else {
            return Vec::new();
        };

        synthetics
            .iter()
            .filter_map(|synthetic| self.synthetic_quote_from_update(synthetic, update))
            .collect()
    }

    fn synthetic_quote_from_update(
        &self,
        synthetic: &SyntheticInstrument,
        update: QuoteTick,
    ) -> Option<QuoteTick> {
        let cache = self.cache.borrow();
        let mut bid_inputs = Vec::with_capacity(synthetic.components.len());
        let mut ask_inputs = Vec::with_capacity(synthetic.components.len());

        for instrument_id in &synthetic.components {
            let (bid_price, ask_price) = if *instrument_id == update.instrument_id {
                (update.bid_price, update.ask_price)
            } else {
                let Some(component_quote) = cache.quote(instrument_id) else {
                    log::warn!(
                        "Cannot calculate synthetic instrument {} price, no quotes for {} yet",
                        synthetic.id,
                        instrument_id,
                    );
                    return None;
                };

                (component_quote.bid_price, component_quote.ask_price)
            };

            bid_inputs.push(bid_price.as_f64());
            ask_inputs.push(ask_price.as_f64());
        }

        drop(cache);

        let bid_price = match synthetic.calculate(&bid_inputs) {
            Ok(price) => price,
            Err(e) => {
                log::error!(
                    "Cannot calculate synthetic instrument {} bid price: {e}",
                    synthetic.id
                );
                return None;
            }
        };

        let ask_price = match synthetic.calculate(&ask_inputs) {
            Ok(price) => price,
            Err(e) => {
                log::error!(
                    "Cannot calculate synthetic instrument {} ask price: {e}",
                    synthetic.id
                );
                return None;
            }
        };

        let size_one = Quantity::from(1);

        Some(QuoteTick::new(
            synthetic.id,
            bid_price,
            ask_price,
            size_one,
            size_one,
            update.ts_event,
            self.clock.borrow().timestamp_ns(),
        ))
    }

    pub(super) fn synthetic_trades_from_trade(&self, update: TradeTick) -> Vec<TradeTick> {
        let Some(synthetics) = self.synthetic_trade_feeds.get(&update.instrument_id) else {
            return Vec::new();
        };

        synthetics
            .iter()
            .filter_map(|synthetic| self.synthetic_trade_from_update(synthetic, update))
            .collect()
    }

    fn synthetic_trade_from_update(
        &self,
        synthetic: &SyntheticInstrument,
        update: TradeTick,
    ) -> Option<TradeTick> {
        let cache = self.cache.borrow();
        let mut inputs = Vec::with_capacity(synthetic.components.len());

        for instrument_id in &synthetic.components {
            let price = if *instrument_id == update.instrument_id {
                update.price
            } else {
                let Some(component_trade) = cache.trade(instrument_id) else {
                    log::warn!(
                        "Cannot calculate synthetic instrument {} price, no trades for {} yet",
                        synthetic.id,
                        instrument_id,
                    );
                    return None;
                };

                component_trade.price
            };

            inputs.push(price.as_f64());
        }

        drop(cache);

        let price = match synthetic.calculate(&inputs) {
            Ok(price) => price,
            Err(e) => {
                log::error!(
                    "Cannot calculate synthetic instrument {} trade price: {e}",
                    synthetic.id
                );
                return None;
            }
        };

        Some(TradeTick::new(
            synthetic.id,
            price,
            Quantity::from(1),
            update.aggressor_side,
            update.trade_id,
            update.ts_event,
            self.clock.borrow().timestamp_ns(),
        ))
    }

    pub(super) fn subscribe_synthetic_quotes(&mut self, instrument_id: InstrumentId) {
        let synthetic = match self.cache.borrow().try_synthetic(&instrument_id).cloned() {
            Ok(synthetic) => synthetic,
            Err(e) => {
                log::error!("Cannot subscribe to `QuoteTick` data for synthetic instrument: {e}");
                return;
            }
        };

        if let Some(owners) = self.subscribed_synthetic_quotes.get_mut(&instrument_id) {
            *owners += 1;
            return;
        }

        self.subscribed_synthetic_quotes.insert(instrument_id, 1);

        for component_id in &synthetic.components {
            let synthetics = self.synthetic_quote_feeds.entry(*component_id).or_default();
            if !synthetics
                .iter()
                .any(|registered| registered.id == synthetic.id)
            {
                synthetics.push(synthetic.clone());
            }
        }
    }

    pub(super) fn subscribe_synthetic_trades(&mut self, instrument_id: InstrumentId) {
        let synthetic = match self.cache.borrow().try_synthetic(&instrument_id).cloned() {
            Ok(synthetic) => synthetic,
            Err(e) => {
                log::error!("Cannot subscribe to `TradeTick` data for synthetic instrument: {e}");
                return;
            }
        };

        if let Some(owners) = self.subscribed_synthetic_trades.get_mut(&instrument_id) {
            *owners += 1;
            return;
        }

        self.subscribed_synthetic_trades.insert(instrument_id, 1);

        for component_id in &synthetic.components {
            let synthetics = self.synthetic_trade_feeds.entry(*component_id).or_default();
            if !synthetics
                .iter()
                .any(|registered| registered.id == synthetic.id)
            {
                synthetics.push(synthetic.clone());
            }
        }
    }

    pub(super) fn unsubscribe_synthetic_quotes(&mut self, instrument_id: InstrumentId) {
        let Some(owners) = self.subscribed_synthetic_quotes.get_mut(&instrument_id) else {
            log::warn!("Cannot unsubscribe from synthetic `QuoteTick` data: not subscribed");
            return;
        };

        if *owners > 1 {
            *owners -= 1;
            return;
        }

        self.subscribed_synthetic_quotes.remove(&instrument_id);

        self.synthetic_quote_feeds.retain(|_, synthetics| {
            synthetics.retain(|synthetic| synthetic.id != instrument_id);
            !synthetics.is_empty()
        });
    }

    pub(super) fn unsubscribe_synthetic_trades(&mut self, instrument_id: InstrumentId) {
        let Some(owners) = self.subscribed_synthetic_trades.get_mut(&instrument_id) else {
            log::warn!("Cannot unsubscribe from synthetic `TradeTick` data: not subscribed");
            return;
        };

        if *owners > 1 {
            *owners -= 1;
            return;
        }

        self.subscribed_synthetic_trades.remove(&instrument_id);

        self.synthetic_trade_feeds.retain(|_, synthetics| {
            synthetics.retain(|synthetic| synthetic.id != instrument_id);
            !synthetics.is_empty()
        });
    }
}
