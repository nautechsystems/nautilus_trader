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

#[cfg(feature = "indicators")]
use std::cell::RefCell;
use std::{
    any::Any,
    error::Error,
    fmt::{Debug, Display},
    rc::Rc,
};

use ahash::AHashMap;
#[cfg(feature = "indicators")]
use nautilus_indicators::indicator::Indicator;
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, QuoteTick, TradeTick},
    identifiers::InstrumentId,
};

/// Shared indicator handle used by actor and strategy registration.
pub type SharedActorIndicator = Rc<dyn ActorIndicator>;

/// Indicator callback interface used by the actor core.
pub trait ActorIndicator: Any {
    /// Returns a stable key used to de-duplicate indicator registrations.
    fn key(&self) -> usize;

    /// Returns this indicator as [`Any`] for adapter-specific downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Checks if the indicator is initialized.
    ///
    /// # Errors
    ///
    /// Returns an error if the indicator cannot report readiness.
    fn initialized(&self) -> anyhow::Result<bool>;

    /// Handles a quote tick.
    ///
    /// # Errors
    ///
    /// Returns an error if the indicator cannot handle the quote tick.
    fn handle_quote(&self, quote: &QuoteTick) -> anyhow::Result<()>;

    /// Handles a trade tick.
    ///
    /// # Errors
    ///
    /// Returns an error if the indicator cannot handle the trade tick.
    fn handle_trade(&self, trade: &TradeTick) -> anyhow::Result<()>;

    /// Handles a bar.
    ///
    /// # Errors
    ///
    /// Returns an error if the indicator cannot handle the bar.
    fn handle_bar(&self, bar: &Bar) -> anyhow::Result<()>;
}

#[cfg(feature = "indicators")]
impl<T> ActorIndicator for RefCell<T>
where
    T: Indicator + 'static,
{
    fn key(&self) -> usize {
        std::ptr::from_ref(self).cast::<()>() as usize
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn initialized(&self) -> anyhow::Result<bool> {
        Ok(self.borrow().initialized())
    }

    fn handle_quote(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.borrow_mut().handle_quote(quote)
    }

    fn handle_trade(&self, trade: &TradeTick) -> anyhow::Result<()> {
        self.borrow_mut().handle_trade(trade);
        Ok(())
    }

    fn handle_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        self.borrow_mut().handle_bar(bar);
        Ok(())
    }
}

/// Registry for actor and strategy indicator callbacks.
#[derive(Clone, Default)]
#[allow(
    clippy::struct_field_names,
    reason = "indicator-prefixed fields denote distinct indicator collections"
)]
pub struct Indicators {
    indicators: Vec<SharedActorIndicator>,
    indicators_for_quotes: AHashMap<InstrumentId, Vec<SharedActorIndicator>>,
    indicators_for_trades: AHashMap<InstrumentId, Vec<SharedActorIndicator>>,
    indicators_for_bars: AHashMap<(InstrumentId, BarSpecification), Vec<SharedActorIndicator>>,
}

impl Debug for Indicators {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Indicators))
            .field("indicators", &self.indicators.len())
            .field("indicators_for_quotes", &self.indicators_for_quotes.len())
            .field("indicators_for_trades", &self.indicators_for_trades.len())
            .field("indicators_for_bars", &self.indicators_for_bars.len())
            .finish()
    }
}

impl Indicators {
    /// Returns the registered indicators.
    #[must_use]
    pub fn registered_indicators(&self) -> Vec<SharedActorIndicator> {
        self.indicators.clone()
    }

    /// Returns whether all registered indicators are initialized.
    ///
    /// # Errors
    ///
    /// Returns an error if a registered indicator cannot report readiness.
    pub fn initialized(&self) -> anyhow::Result<bool> {
        if self.indicators.is_empty() {
            return Ok(false);
        }

        for indicator in &self.indicators {
            if !indicator.initialized()? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Registers an indicator to receive quote ticks for an instrument.
    pub fn register_indicator_for_quote_ticks(
        &mut self,
        instrument_id: InstrumentId,
        indicator: SharedActorIndicator,
    ) {
        self.register_indicator(indicator.clone());
        self.register_by_key(instrument_id, indicator, IndicatorKind::Quote);
    }

    /// Registers an indicator to receive trade ticks for an instrument.
    pub fn register_indicator_for_trade_ticks(
        &mut self,
        instrument_id: InstrumentId,
        indicator: SharedActorIndicator,
    ) {
        self.register_indicator(indicator.clone());
        self.register_by_key(instrument_id, indicator, IndicatorKind::Trade);
    }

    /// Registers an indicator to receive bars for a bar type.
    pub fn register_indicator_for_bars(
        &mut self,
        bar_type: BarType,
        indicator: SharedActorIndicator,
    ) {
        self.register_indicator(indicator.clone());
        self.register_bar(bar_type.id_spec_key(), indicator);
    }

    /// Handles a quote tick with registered indicators.
    ///
    /// # Errors
    ///
    /// Returns an error if a registered indicator cannot handle the quote tick.
    pub fn handle_quote(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        let mut failures = IndicatorFailures::default();
        self.collect_quote_failures(quote, &mut failures);
        failures.into_result()
    }

    /// Handles quote ticks with registered indicators.
    ///
    /// # Errors
    ///
    /// Returns an error if a registered indicator cannot handle a quote tick.
    pub fn handle_quotes(&self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        let mut failures = IndicatorFailures::default();

        for quote in quotes {
            self.collect_quote_failures(quote, &mut failures);
        }

        failures.into_result()
    }

    /// Handles a trade tick with registered indicators.
    ///
    /// # Errors
    ///
    /// Returns an error if a registered indicator cannot handle the trade tick.
    pub fn handle_trade(&self, trade: &TradeTick) -> anyhow::Result<()> {
        let mut failures = IndicatorFailures::default();
        self.collect_trade_failures(trade, &mut failures);
        failures.into_result()
    }

    /// Handles trade ticks with registered indicators.
    ///
    /// # Errors
    ///
    /// Returns an error if a registered indicator cannot handle a trade tick.
    pub fn handle_trades(&self, trades: &[TradeTick]) -> anyhow::Result<()> {
        let mut failures = IndicatorFailures::default();

        for trade in trades {
            self.collect_trade_failures(trade, &mut failures);
        }

        failures.into_result()
    }

    /// Handles a bar with registered indicators.
    ///
    /// # Errors
    ///
    /// Returns an error if a registered indicator cannot handle the bar.
    pub fn handle_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        let mut failures = IndicatorFailures::default();
        self.collect_bar_failures(bar, &mut failures);
        failures.into_result()
    }

    /// Handles bars with registered indicators.
    ///
    /// # Errors
    ///
    /// Returns an error if a registered indicator cannot handle a bar.
    pub fn handle_bars(&self, bars: &[Bar]) -> anyhow::Result<()> {
        let mut failures = IndicatorFailures::default();

        for bar in bars {
            self.collect_bar_failures(bar, &mut failures);
        }

        failures.into_result()
    }

    fn collect_quote_failures(&self, quote: &QuoteTick, failures: &mut IndicatorFailures) {
        if let Some(indicators) = self.indicators_for_quotes.get(&quote.instrument_id) {
            for indicator in indicators {
                if let Err(e) = indicator.handle_quote(quote) {
                    failures.push(&e);
                }
            }
        }
    }

    fn collect_trade_failures(&self, trade: &TradeTick, failures: &mut IndicatorFailures) {
        if let Some(indicators) = self.indicators_for_trades.get(&trade.instrument_id) {
            for indicator in indicators {
                if let Err(e) = indicator.handle_trade(trade) {
                    failures.push(&e);
                }
            }
        }
    }

    fn collect_bar_failures(&self, bar: &Bar, failures: &mut IndicatorFailures) {
        if let Some(indicators) = self.indicators_for_bars.get(&bar.bar_type.id_spec_key()) {
            for indicator in indicators {
                if let Err(e) = indicator.handle_bar(bar) {
                    failures.push(&e);
                }
            }
        }
    }

    fn register_indicator(&mut self, indicator: SharedActorIndicator) {
        if !contains_indicator(&self.indicators, &indicator) {
            self.indicators.push(indicator);
        }
    }

    fn register_by_key(
        &mut self,
        instrument_id: InstrumentId,
        indicator: SharedActorIndicator,
        kind: IndicatorKind,
    ) {
        let indicators = match kind {
            IndicatorKind::Quote => self.indicators_for_quotes.entry(instrument_id).or_default(),
            IndicatorKind::Trade => self.indicators_for_trades.entry(instrument_id).or_default(),
        };

        if !contains_indicator(indicators, &indicator) {
            indicators.push(indicator);
        }
    }

    fn register_bar(
        &mut self,
        bar_key: (InstrumentId, BarSpecification),
        indicator: SharedActorIndicator,
    ) {
        let indicators = self.indicators_for_bars.entry(bar_key).or_default();

        if !contains_indicator(indicators, &indicator) {
            indicators.push(indicator);
        }
    }
}

const MAX_FAILURE_DETAILS: usize = 3;

#[derive(Debug, Default)]
struct IndicatorFailures {
    total: usize,
    details: Vec<String>,
}

impl IndicatorFailures {
    fn push(&mut self, e: &anyhow::Error) {
        self.total += 1;

        // Retain distinct messages only: one indicator failing on every item of a
        // batch would otherwise fill every slot and hide a different failure.
        let detail = e.to_string();

        if self.details.len() < MAX_FAILURE_DETAILS && !self.details.contains(&detail) {
            self.details.push(detail);
        }
    }

    fn into_result(self) -> anyhow::Result<()> {
        if self.total == 0 {
            Ok(())
        } else {
            Err(anyhow::Error::new(self))
        }
    }
}

impl Display for IndicatorFailures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = if self.total == 1 {
            "indicator failure"
        } else {
            "indicator failures"
        };
        write!(f, "{} {label}: {}", self.total, self.details.join("; "))?;

        if self.total > self.details.len() {
            let omitted = self.total - self.details.len();
            let noun = if omitted == 1 { "failure" } else { "failures" };
            write!(f, "; {omitted} additional {noun} omitted")?;
        }

        Ok(())
    }
}

impl Error for IndicatorFailures {}

#[derive(Clone, Copy)]
enum IndicatorKind {
    Quote,
    Trade,
}

fn contains_indicator(
    indicators: &[SharedActorIndicator],
    indicator: &SharedActorIndicator,
) -> bool {
    let indicator_key = indicator.key();
    indicators
        .iter()
        .any(|registered| registered.key() == indicator_key)
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        cell::Cell,
        rc::Rc,
        str::FromStr,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use nautilus_model::data::{Bar, BarType, QuoteTick, TradeTick};
    use rstest::rstest;

    use super::{ActorIndicator, Indicators, SharedActorIndicator};

    static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);

    #[derive(Debug)]
    struct TrackingIndicator {
        key: usize,
        initialized: Cell<bool>,
        quotes: Cell<usize>,
        trades: Cell<usize>,
        bars: Cell<usize>,
    }

    impl TrackingIndicator {
        fn new() -> Self {
            Self {
                key: NEXT_KEY.fetch_add(1, Ordering::Relaxed),
                initialized: Cell::new(false),
                quotes: Cell::new(0),
                trades: Cell::new(0),
                bars: Cell::new(0),
            }
        }

        fn set_initialized(&self, initialized: bool) {
            self.initialized.set(initialized);
        }
    }

    impl ActorIndicator for TrackingIndicator {
        fn key(&self) -> usize {
            self.key
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn initialized(&self) -> anyhow::Result<bool> {
            Ok(self.initialized.get())
        }

        fn handle_quote(&self, _quote: &QuoteTick) -> anyhow::Result<()> {
            self.quotes.set(self.quotes.get() + 1);
            Ok(())
        }

        fn handle_trade(&self, _trade: &TradeTick) -> anyhow::Result<()> {
            self.trades.set(self.trades.get() + 1);
            Ok(())
        }

        fn handle_bar(&self, _bar: &Bar) -> anyhow::Result<()> {
            self.bars.set(self.bars.get() + 1);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct ErrorIndicator {
        key: usize,
        message: String,
    }

    impl ErrorIndicator {
        fn new() -> Self {
            Self::with_message("indicator failed")
        }

        fn with_message(message: &str) -> Self {
            Self {
                key: NEXT_KEY.fetch_add(1, Ordering::Relaxed),
                message: message.to_string(),
            }
        }
    }

    impl ActorIndicator for ErrorIndicator {
        fn key(&self) -> usize {
            self.key
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn initialized(&self) -> anyhow::Result<bool> {
            Ok(true)
        }

        fn handle_quote(&self, _quote: &QuoteTick) -> anyhow::Result<()> {
            anyhow::bail!("{}", self.message);
        }

        fn handle_trade(&self, _trade: &TradeTick) -> anyhow::Result<()> {
            anyhow::bail!("{}", self.message);
        }

        fn handle_bar(&self, _bar: &Bar) -> anyhow::Result<()> {
            anyhow::bail!("{}", self.message);
        }
    }

    #[rstest]
    fn test_register_indicator_routes_quote_trade_and_bar_with_deduplication() {
        let mut indicators = Indicators::default();
        let indicator = Rc::new(TrackingIndicator::new());
        let registered: SharedActorIndicator = indicator.clone();
        let quote = QuoteTick::default();
        let trade = TradeTick::default();
        let bar = Bar::default();
        let external_bar_type = BarType::from_str(&format!(
            "{}-1-MINUTE-LAST-EXTERNAL",
            bar.bar_type.instrument_id()
        ))
        .unwrap();

        indicators.register_indicator_for_quote_ticks(quote.instrument_id, registered.clone());
        indicators.register_indicator_for_quote_ticks(quote.instrument_id, registered.clone());
        indicators.register_indicator_for_trade_ticks(trade.instrument_id, registered.clone());
        indicators.register_indicator_for_trade_ticks(trade.instrument_id, registered.clone());
        indicators.register_indicator_for_bars(external_bar_type, registered.clone());
        indicators.register_indicator_for_bars(external_bar_type, registered);

        indicators.handle_quote(&quote).unwrap();
        indicators.handle_trade(&trade).unwrap();
        indicators.handle_bar(&bar).unwrap();

        assert_eq!(indicators.registered_indicators().len(), 1);
        assert_eq!(indicator.quotes.get(), 1);
        assert_eq!(indicator.trades.get(), 1);
        assert_eq!(indicator.bars.get(), 1);
    }

    #[rstest]
    fn test_initialized_requires_all_registered_indicators() {
        let mut indicators = Indicators::default();
        let first = Rc::new(TrackingIndicator::new());
        let second = Rc::new(TrackingIndicator::new());
        let quote = QuoteTick::default();

        indicators.register_indicator_for_quote_ticks(quote.instrument_id, first.clone());
        indicators.register_indicator_for_quote_ticks(quote.instrument_id, second.clone());

        first.set_initialized(true);

        assert!(!indicators.initialized().unwrap());

        second.set_initialized(true);

        assert!(indicators.initialized().unwrap());
    }

    #[rstest]
    fn test_handle_quote_propagates_indicator_error() {
        let mut indicators = Indicators::default();
        let indicator = Rc::new(ErrorIndicator::new());
        let quote = QuoteTick::default();

        indicators.register_indicator_for_quote_ticks(quote.instrument_id, indicator);

        let err = indicators.handle_quote(&quote).unwrap_err();

        assert!(err.to_string().contains("indicator failed"));
    }

    #[rstest]
    fn test_handle_quote_continues_after_indicator_error() {
        let mut indicators = Indicators::default();
        let before = Rc::new(TrackingIndicator::new());
        let failing = Rc::new(ErrorIndicator::new());
        let after = Rc::new(TrackingIndicator::new());
        let quote = QuoteTick::default();

        indicators.register_indicator_for_quote_ticks(quote.instrument_id, before.clone());
        indicators.register_indicator_for_quote_ticks(quote.instrument_id, failing);
        indicators.register_indicator_for_quote_ticks(quote.instrument_id, after.clone());

        assert!(indicators.handle_quote(&quote).is_err());
        assert_eq!(before.quotes.get(), 1);
        assert_eq!(after.quotes.get(), 1);
    }

    #[rstest]
    fn test_handle_trade_continues_after_indicator_error() {
        let mut indicators = Indicators::default();
        let before = Rc::new(TrackingIndicator::new());
        let failing = Rc::new(ErrorIndicator::new());
        let after = Rc::new(TrackingIndicator::new());
        let trade = TradeTick::default();

        indicators.register_indicator_for_trade_ticks(trade.instrument_id, before.clone());
        indicators.register_indicator_for_trade_ticks(trade.instrument_id, failing);
        indicators.register_indicator_for_trade_ticks(trade.instrument_id, after.clone());

        assert!(indicators.handle_trade(&trade).is_err());
        assert_eq!(before.trades.get(), 1);
        assert_eq!(after.trades.get(), 1);
    }

    #[rstest]
    fn test_handle_bar_continues_after_indicator_error() {
        let mut indicators = Indicators::default();
        let before = Rc::new(TrackingIndicator::new());
        let failing = Rc::new(ErrorIndicator::new());
        let after = Rc::new(TrackingIndicator::new());
        let bar = Bar::default();

        indicators.register_indicator_for_bars(bar.bar_type, before.clone());
        indicators.register_indicator_for_bars(bar.bar_type, failing);
        indicators.register_indicator_for_bars(bar.bar_type, after.clone());

        assert!(indicators.handle_bar(&bar).is_err());
        assert_eq!(before.bars.get(), 1);
        assert_eq!(after.bars.get(), 1);
    }

    #[rstest]
    fn test_handle_quotes_continues_after_indicator_error() {
        let mut indicators = Indicators::default();
        let before = Rc::new(TrackingIndicator::new());
        let failing = Rc::new(ErrorIndicator::new());
        let after = Rc::new(TrackingIndicator::new());
        let quotes = [QuoteTick::default(), QuoteTick::default()];

        indicators.register_indicator_for_quote_ticks(quotes[0].instrument_id, before.clone());
        indicators.register_indicator_for_quote_ticks(quotes[0].instrument_id, failing);
        indicators.register_indicator_for_quote_ticks(quotes[0].instrument_id, after.clone());

        assert!(indicators.handle_quotes(&quotes).is_err());
        assert_eq!(before.quotes.get(), 2);
        assert_eq!(after.quotes.get(), 2);
    }

    #[rstest]
    fn test_handle_trades_continues_after_indicator_error() {
        let mut indicators = Indicators::default();
        let before = Rc::new(TrackingIndicator::new());
        let failing = Rc::new(ErrorIndicator::new());
        let after = Rc::new(TrackingIndicator::new());
        let trades = [TradeTick::default(), TradeTick::default()];

        indicators.register_indicator_for_trade_ticks(trades[0].instrument_id, before.clone());
        indicators.register_indicator_for_trade_ticks(trades[0].instrument_id, failing);
        indicators.register_indicator_for_trade_ticks(trades[0].instrument_id, after.clone());

        assert!(indicators.handle_trades(&trades).is_err());
        assert_eq!(before.trades.get(), 2);
        assert_eq!(after.trades.get(), 2);
    }

    #[rstest]
    fn test_handle_bars_continues_after_indicator_error() {
        let mut indicators = Indicators::default();
        let before = Rc::new(TrackingIndicator::new());
        let failing = Rc::new(ErrorIndicator::new());
        let after = Rc::new(TrackingIndicator::new());
        let bars = [Bar::default(), Bar::default()];

        indicators.register_indicator_for_bars(bars[0].bar_type, before.clone());
        indicators.register_indicator_for_bars(bars[0].bar_type, failing);
        indicators.register_indicator_for_bars(bars[0].bar_type, after.clone());

        assert!(indicators.handle_bars(&bars).is_err());
        assert_eq!(before.bars.get(), 2);
        assert_eq!(after.bars.get(), 2);
    }

    #[rstest]
    fn test_handle_quote_error_collapses_identical_details_and_counts_all() {
        let mut indicators = Indicators::default();
        let first = Rc::new(ErrorIndicator::new());
        let second = Rc::new(ErrorIndicator::new());
        let third = Rc::new(ErrorIndicator::new());
        let fourth = Rc::new(ErrorIndicator::new());
        let quote = QuoteTick::default();

        indicators.register_indicator_for_quote_ticks(quote.instrument_id, first);
        indicators.register_indicator_for_quote_ticks(quote.instrument_id, second);
        indicators.register_indicator_for_quote_ticks(quote.instrument_id, third);
        indicators.register_indicator_for_quote_ticks(quote.instrument_id, fourth);

        let err = indicators.handle_quote(&quote).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("4 indicator failures"));
        assert!(message.contains("3 additional failures omitted"));
    }

    #[rstest]
    fn test_handle_quote_error_retains_distinct_details_up_to_the_bound() {
        let mut indicators = Indicators::default();
        let quote = QuoteTick::default();

        for message in [
            "first failure",
            "second failure",
            "third failure",
            "fourth failure",
        ] {
            indicators.register_indicator_for_quote_ticks(
                quote.instrument_id,
                Rc::new(ErrorIndicator::with_message(message)),
            );
        }

        let err = indicators.handle_quote(&quote).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("4 indicator failures"));
        assert!(message.contains("first failure"));
        assert!(message.contains("second failure"));
        assert!(message.contains("third failure"));
        assert!(!message.contains("fourth failure"));
        assert!(message.contains("1 additional failure omitted"));
    }
}
