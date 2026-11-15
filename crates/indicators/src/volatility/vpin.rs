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

//! Volume-synchronized probability of informed trading (VPIN).

use std::{
    cmp::Ordering,
    collections::VecDeque,
    fmt::{Debug, Display},
};

use nautilus_core::correctness::FAILED;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::AggressorSide,
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};

use crate::{
    indicator::Indicator,
    support::{MAX_PERIOD, ShiftedMoments},
};

/// The default number of bar close changes used to estimate sigma for bulk volume classification.
pub const VPIN_DEFAULT_SIGMA_WINDOW: usize = 50;

/// Volume-synchronized probability of informed trading (VPIN).
///
/// Easley, Lopez de Prado and O'Hara (2012), "Flow Toxicity and Liquidity in a High-frequency
/// World", Review of Financial Studies 25(5). Classified volume fills consecutive buckets of
/// exactly `bucket_volume` units. A print that overflows a bucket is split, keeping its buy share,
/// and the remainder carries into the next bucket. The value is the mean order imbalance
/// `|V_buy - V_sell| / bucket_volume` over the last `window` full buckets, so it lies in `[0, 1]`.
///
/// Volume classification depends on the input type. Trades and bars for the same market activity
/// can produce different values. Use one input source per event to avoid counting volume twice.
///
/// - Trades use the venue aggressor side. A trade with no aggressor falls back to the tick rule
///   (an uptick is a buy, a downtick is a sell, and an unchanged price repeats the last nonzero
///   tick direction). Unsigned trades before the first price change contribute no volume.
/// - Bars use bulk volume classification. The buy share of the bar volume is `Phi(dP / sigma)`,
///   where `dP` is the change from the previous close and `sigma` the sample standard deviation of
///   the last `sigma_window` close changes, including the current one. Pass `sigma_window` to
///   override the default of 50 changes. Bars contribute no volume until the window fills.
///   Any zero-variance window splits the bar volume 50/50.
/// - [`Vpin::update_raw`] uses the buy and sell volume supplied by the caller.
///
/// `count` tracks completed buckets and saturates at the platform counter limit. The indicator
/// initializes after `window` buckets close.
///
/// Bar and trade handlers use fixed-point quantities when `bucket_volume` fits the model's
/// fixed-point scale. Updates that take floating-point volumes use the supplied `f64` values.
/// A decimal sum that would fill a bucket can fall short when supplied as binary floats.
/// Once fixed-point accounting begins, floating-point volumes contribute the same fraction of
/// a bucket in fixed-point units. Rounding when mixing volume sources can delay bucket completion.
/// Bucket sizes outside the model scale use floating-point rounding for all volume inputs.
///
/// The paper sizes buckets at one fiftieth of average daily volume and averages over 50 buckets.
/// VPIN is a research indicator with contested predictive value. Thresholds are strategy-specific.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct Vpin {
    bucket_volume: f64,
    bucket_quantity: Option<Quantity>,
    window: usize,
    sigma_window: usize,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    bucket_fill: CompensatedSum,
    bucket_fill_precision: Option<u8>,
    bucket_imbalance: CompensatedSum,
    imbalances: VecDeque<f64>,
    imbalance_sum: CompensatedSum,
    last_trade_price: Option<InputPrice>,
    last_tick_is_buy: Option<bool>,
    last_close: Option<InputPrice>,
    close_changes: VecDeque<CloseChange>,
    equal_changes: usize,
    moments: ShiftedMoments,
}

impl Display for Vpin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.bucket_volume,
            self.window,
            self.sigma_window,
        )
    }
}

impl Indicator for Vpin {
    fn name(&self) -> String {
        stringify!(Vpin).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        if !trade.price.is_sentinel() && trade.size.is_positive() {
            self.update_tick(
                InputPrice::Fixed(trade.price),
                InputVolume::Fixed(trade.size),
                trade.aggressor_side,
            );
        }
    }

    fn handle_bar(&mut self, bar: &Bar) {
        if !bar.close.is_sentinel() && !bar.volume.is_undefined() {
            self.update_close(InputPrice::Fixed(bar.close), InputVolume::Fixed(bar.volume));
        }
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.initialized = false;
        self.has_inputs = false;
        self.bucket_fill = CompensatedSum::default();
        self.bucket_fill_precision = None;
        self.bucket_imbalance = CompensatedSum::default();
        self.imbalances.clear();
        self.imbalance_sum = CompensatedSum::default();
        self.last_trade_price = None;
        self.last_tick_is_buy = None;
        self.last_close = None;
        self.close_changes.clear();
        self.equal_changes = 0;
        self.moments.reset();
    }
}

impl Vpin {
    /// Creates a new [`Vpin`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `bucket_volume` is not finite and positive, if `window` is zero or above the
    /// maximum period, or if `sigma_window` is below 2 or above the maximum period.
    #[must_use]
    pub fn new(bucket_volume: f64, window: usize, sigma_window: Option<usize>) -> Self {
        Self::new_checked(bucket_volume, window, sigma_window).expect(FAILED)
    }

    pub(crate) fn new_checked(
        bucket_volume: f64,
        window: usize,
        sigma_window: Option<usize>,
    ) -> anyhow::Result<Self> {
        let sigma_window = sigma_window.unwrap_or(VPIN_DEFAULT_SIGMA_WINDOW);
        anyhow::ensure!(
            bucket_volume.is_finite() && bucket_volume > 0.0,
            "Vpin: bucket_volume must be finite and > 0 (received {bucket_volume})"
        );
        anyhow::ensure!(window <= MAX_PERIOD, "window cannot exceed {MAX_PERIOD}");
        anyhow::ensure!(window > 0, "Vpin: window must be > 0 (received {window})");
        anyhow::ensure!(
            sigma_window <= MAX_PERIOD,
            "sigma_window cannot exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            sigma_window >= 2,
            "Vpin: sigma_window must be >= 2 (received {sigma_window})"
        );
        Ok(Self {
            bucket_volume,
            bucket_quantity: bucket_volume
                .to_string()
                .parse::<Quantity>()
                .ok()
                .filter(Quantity::is_positive),
            window,
            sigma_window,
            value: 0.0,
            count: 0,
            initialized: false,
            has_inputs: false,
            bucket_fill: CompensatedSum::default(),
            bucket_fill_precision: None,
            bucket_imbalance: CompensatedSum::default(),
            imbalances: VecDeque::with_capacity(window.min(1024)),
            imbalance_sum: CompensatedSum::default(),
            last_trade_price: None,
            last_tick_is_buy: None,
            last_close: None,
            close_changes: VecDeque::with_capacity(sigma_window.min(1024)),
            equal_changes: 0,
            moments: ShiftedMoments::new(),
        })
    }

    /// Returns the volume of a complete bucket.
    #[must_use]
    pub const fn bucket_volume(&self) -> f64 {
        self.bucket_volume
    }

    /// Returns the number of completed buckets in the VPIN average.
    #[must_use]
    pub const fn window(&self) -> usize {
        self.window
    }

    /// Returns the number of bar close changes used to estimate the sample standard deviation.
    #[must_use]
    pub const fn sigma_window(&self) -> usize {
        self.sigma_window
    }

    /// Updates the indicator with a trade price, size and aggressor side.
    ///
    /// Trades with [`AggressorSide::NoAggressor`] are signed by the tick rule. Every trade with a
    /// finite price and positive size advances the tick rule, so its history stays complete
    /// whichever side source signs a given trade.
    ///
    /// When comparing against a model price, the tick rule uses the float's shortest decimal
    /// representation if it fits in a [`Price`]. Otherwise it uses the binary value.
    pub fn update_trade(&mut self, price: f64, size: f64, aggressor_side: AggressorSide) {
        if price.is_finite() && size.is_finite() && size > 0.0 {
            self.update_tick(
                InputPrice::Float(price),
                InputVolume::Float(size),
                aggressor_side,
            );
        }
    }

    /// Updates the indicator with a bar close and volume using bulk volume classification.
    ///
    /// Close changes are computed in floating point. Use [`Indicator::handle_bar`] with
    /// fixed-point prices to preserve equal decimal steps.
    pub fn update_bar(&mut self, close: f64, volume: f64) {
        if close.is_finite() && volume.is_finite() && volume >= 0.0 {
            self.update_close(InputPrice::Float(close), InputVolume::Float(volume));
        }
    }

    /// Updates the indicator with buy and sell volume classified by the caller.
    ///
    /// Both sides may be nonzero. When an input spans several buckets, each portion keeps the
    /// input's buy share. Volumes aggregated by side carry no trade order.
    pub fn update_raw(&mut self, buy_volume: f64, sell_volume: f64) {
        if !buy_volume.is_finite()
            || !sell_volume.is_finite()
            || buy_volume < 0.0
            || sell_volume < 0.0
        {
            return;
        }

        let largest = buy_volume.max(sell_volume);
        if largest > 0.0 {
            let smallest = buy_volume.min(sell_volume);
            let imbalance = ((buy_volume - sell_volume) / largest) / (1.0 + smallest / largest);
            self.update_volume(InputVolume::Float(buy_volume), imbalance);
            self.update_volume(InputVolume::Float(sell_volume), imbalance);
        }
    }

    fn update_tick(&mut self, price: InputPrice, size: InputVolume, aggressor_side: AggressorSide) {
        self.has_inputs = true;

        if let Some(previous) = self.last_trade_price.replace(price) {
            match price.compare(previous) {
                Ordering::Greater => self.last_tick_is_buy = Some(true),
                Ordering::Less => self.last_tick_is_buy = Some(false),
                Ordering::Equal => {}
            }
        }

        let is_buy = match aggressor_side {
            AggressorSide::Buy => Some(true),
            AggressorSide::Sell => Some(false),
            AggressorSide::NoAggressor => self.last_tick_is_buy,
        };

        if let Some(is_buy) = is_buy {
            self.update_volume(size, if is_buy { 1.0 } else { -1.0 });
        }
    }

    fn update_close(&mut self, close: InputPrice, volume: InputVolume) {
        self.has_inputs = true;

        let Some(previous) = self.last_close.replace(close) else {
            return;
        };

        let change = close.change_from(previous);
        if !change.value.is_finite() {
            self.close_changes.clear();
            self.equal_changes = 0;
            self.moments.reset();
            return;
        }

        self.equal_changes = if self
            .close_changes
            .back()
            .is_some_and(|old| change.same_as(*old))
        {
            self.equal_changes.saturating_add(1)
        } else {
            1
        };

        let evicted = if self.close_changes.len() == self.sigma_window {
            self.close_changes.pop_front()
        } else {
            None
        };

        if let Some(old) = evicted {
            self.moments.evict(old.value);
        }
        self.close_changes.push_back(change);
        self.moments.push(change.value);

        if self.close_changes.len() < self.sigma_window {
            return;
        }

        let dispersion = self.close_dispersion(change, evicted);
        let imbalance = if dispersion > 0.0 {
            let n = self.sigma_window as f64;
            let sigma = dispersion * (n / (n - 1.0)).sqrt();
            2.0 * normal_cdf(change.value / sigma) - 1.0
        } else {
            0.0
        };

        self.update_volume(volume, imbalance);
    }

    fn close_dispersion(&mut self, change: CloseChange, evicted: Option<CloseChange>) -> f64 {
        let mut dispersion = self.moments.std_dev(self.sigma_window);

        // An evicted outlier can leave cancellation error larger than the new window's variance
        if self.equal_changes == self.sigma_window
            || self.moments.needs_reseed(self.sigma_window)
            || evicted.is_some_and(|old| (old.value - change.value).abs() > dispersion * 1024.0)
        {
            self.moments
                .reseed(self.close_changes.iter().map(|change| &change.value));
            dispersion = self.moments.std_dev(self.sigma_window);
        }

        if self.equal_changes >= self.sigma_window {
            0.0
        } else if dispersion <= change.value.abs() * f64::EPSILON * 1024.0 {
            // Close changes can round alike before their much smaller differences are taken
            let n = self.sigma_window as f64;
            let deltas = self.close_changes.iter().map(|old| old.difference(change));
            let mean = deltas.clone().sum::<f64>() / n;
            (deltas.map(|delta| (delta - mean).powi(2)).sum::<f64>() / n).sqrt()
        } else {
            dispersion
        }
    }

    fn update_volume(&mut self, volume: InputVolume, imbalance: f64) {
        let (buckets, remainder, capacity, precision) =
            match (volume, self.bucket_quantity, self.bucket_fill_precision) {
                (InputVolume::Fixed(volume), Some(bucket), previous) => {
                    let precision = volume
                        .precision
                        .max(bucket.precision)
                        .max(previous.unwrap_or(FIXED_PRECISION));
                    let units = quantity_units(volume, precision);
                    let capacity = quantity_units(bucket, precision);

                    (
                        usize::try_from(units / capacity).unwrap_or(usize::MAX),
                        CompensatedSum::from_units(units % capacity),
                        CompensatedSum::from_units(capacity),
                        Some(precision),
                    )
                }
                (InputVolume::Float(volume), Some(bucket), Some(precision)) => {
                    let (buckets, remainder) = bucket_div_rem(volume, self.bucket_volume);
                    let capacity = CompensatedSum::from_units(quantity_units(bucket, precision));
                    let remainder = CompensatedSum::from(remainder).rescale_to_fixed(
                        self.bucket_volume,
                        capacity,
                        10_u128.pow(u32::from(precision)) as f64,
                    );

                    (buckets, remainder, capacity, Some(precision))
                }
                (volume, _, _) => {
                    let (buckets, remainder) =
                        bucket_div_rem(volume.as_float(), self.bucket_volume);

                    (
                        buckets,
                        CompensatedSum::from(remainder),
                        CompensatedSum::from(self.bucket_volume),
                        None,
                    )
                }
            };

        if buckets > 0 || remainder.value > 0.0 {
            self.has_inputs = true;

            if let Some(precision) = precision {
                self.promote_bucket_fill(precision, capacity);
            }

            self.update_buckets(buckets, remainder, capacity, imbalance);
        }
    }

    fn promote_bucket_fill(&mut self, precision: u8, capacity: CompensatedSum) {
        match self.bucket_fill_precision {
            Some(previous) if precision > previous => {
                self.bucket_fill = self
                    .bucket_fill
                    .multiply(10_u128.pow(u32::from(precision - previous)) as f64);
            }
            None => {
                self.bucket_fill = self.bucket_fill.rescale_to_fixed(
                    self.bucket_volume,
                    capacity,
                    10_u128.pow(u32::from(precision)) as f64,
                );
            }
            Some(_) => return,
        }

        self.bucket_fill_precision = Some(precision);
    }

    fn update_buckets(
        &mut self,
        mut buckets: usize,
        remainder: CompensatedSum,
        capacity: CompensatedSum,
        imbalance: f64,
    ) {
        let negative_space = self.bucket_fill.add_sum(-capacity);
        let overflow = negative_space.add_sum(remainder);
        if overflow.is_nonnegative() {
            self.bucket_fill = overflow;
            buckets = buckets.saturating_add(1);
        } else {
            self.bucket_fill = self.bucket_fill.add_sum(remainder);
        }

        if buckets == 0 {
            self.bucket_imbalance = self
                .bucket_imbalance
                .add((remainder.value / capacity.value) * imbalance);
            return;
        }

        let first = self
            .bucket_imbalance
            .add((-negative_space.value / capacity.value) * imbalance)
            .value
            .abs()
            .min(1.0);

        self.push_buckets(first, 1);
        self.push_buckets(imbalance.abs(), buckets - 1);
        self.bucket_imbalance =
            CompensatedSum::from((self.bucket_fill.value / capacity.value) * imbalance);

        if self.imbalances.len() == self.window {
            self.value = (self.imbalance_sum.value / self.window as f64).clamp(0.0, 1.0);
            self.initialized = true;
        }
    }

    fn push_buckets(&mut self, imbalance: f64, count: usize) {
        self.count = self.count.saturating_add(count);
        if count >= self.window {
            self.imbalances.clear();
            self.imbalances.resize(self.window, imbalance);
            let window = self.window as f64;
            let sum = imbalance * window;

            self.imbalance_sum = CompensatedSum {
                value: sum,
                residual: imbalance.mul_add(window, -sum),
            };
        } else {
            let evict = (self.imbalances.len() + count).saturating_sub(self.window);
            for old in self.imbalances.drain(..evict) {
                self.imbalance_sum = self.imbalance_sum.add(-old);
            }

            for _ in 0..count {
                self.imbalances.push_back(imbalance);
                self.imbalance_sum = self.imbalance_sum.add(imbalance);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum InputPrice {
    Fixed(Price),
    Float(f64),
}

impl InputPrice {
    fn compare(self, previous: Self) -> Ordering {
        match (self, previous) {
            (Self::Fixed(price), Self::Fixed(previous)) => price.cmp(&previous),
            (Self::Fixed(price), Self::Float(previous)) => {
                Self::compare_fixed_to_float(price, previous)
            }
            (Self::Float(price), Self::Fixed(previous)) => {
                Self::compare_fixed_to_float(previous, price).reverse()
            }
            (Self::Float(price), Self::Float(previous)) => {
                price.partial_cmp(&previous).unwrap_or(Ordering::Equal)
            }
        }
    }

    fn compare_fixed_to_float(price: Price, float: f64) -> Ordering {
        float.to_string().parse::<Price>().map_or_else(
            |_| {
                Self::fixed_minus_float(price, float)
                    .partial_cmp(&0.0)
                    .unwrap_or(Ordering::Equal)
            },
            |float| price.cmp(&float),
        )
    }

    fn change_from(self, previous: Self) -> CloseChange {
        match (self, previous) {
            (Self::Fixed(close), Self::Fixed(previous)) => {
                let mut scale = close.precision.max(previous.precision).max(FIXED_PRECISION);
                let mut change = price_units(close, scale) - price_units(previous, scale);
                while scale > FIXED_PRECISION && change % 10 == 0 {
                    change /= 10;
                    scale -= 1;
                }

                CloseChange {
                    value: change as f64 / 10_u128.pow(u32::from(scale)) as f64,
                    fixed: Some((change, scale)),
                }
            }
            (Self::Fixed(close), Self::Float(previous)) => CloseChange {
                value: Self::fixed_minus_float(close, previous),
                fixed: None,
            },
            (Self::Float(close), Self::Fixed(previous)) => CloseChange {
                value: -Self::fixed_minus_float(previous, close),
                fixed: None,
            },
            (Self::Float(close), Self::Float(previous)) => CloseChange {
                value: close - previous,
                fixed: None,
            },
        }
    }

    fn fixed_minus_float(price: Price, float: f64) -> f64 {
        let precision = price.precision.max(FIXED_PRECISION);
        let scale = 10_u128.pow(u32::from(precision)) as f64;
        if !(float * scale).is_finite() {
            return -float;
        }

        let units = price_units(price, precision);
        let fixed = CompensatedSum::from_units(units.unsigned_abs());
        let fixed = if units < 0 { -fixed } else { fixed };
        let float = CompensatedSum::from(float).multiply(scale);

        fixed.add_sum(-float).value / scale
    }
}

#[derive(Clone, Copy, Debug)]
struct CloseChange {
    value: f64,
    fixed: Option<(i128, u8)>,
}

impl CloseChange {
    fn same_as(self, other: Self) -> bool {
        match (self.fixed, other.fixed) {
            (Some(left), Some(right)) => left == right,
            _ => self.value.partial_cmp(&other.value) == Some(Ordering::Equal),
        }
    }

    fn difference(self, other: Self) -> f64 {
        match (self.fixed, other.fixed) {
            (Some((left, left_scale)), Some((right, right_scale))) => {
                let scale = left_scale.max(right_scale);
                let left = left * 10_i128.pow(u32::from(scale - left_scale));
                let right = right * 10_i128.pow(u32::from(scale - right_scale));
                (left - right) as f64 / 10_u128.pow(u32::from(scale)) as f64
            }
            _ => self.value - other.value,
        }
    }
}

#[allow(
    clippy::useless_conversion,
    reason = "PriceRaw is i64 in standard precision and i128 in high precision"
)]
fn price_units(price: Price, scale: u8) -> i128 {
    i128::from(price.raw()) * 10_i128.pow(u32::from(scale - price.precision.max(FIXED_PRECISION)))
}

#[derive(Clone, Copy, Debug)]
enum InputVolume {
    Fixed(Quantity),
    Float(f64),
}

impl InputVolume {
    fn as_float(self) -> f64 {
        match self {
            Self::Fixed(volume) => {
                volume.raw() as f64
                    / 10_u128.pow(u32::from(volume.precision.max(FIXED_PRECISION))) as f64
            }
            Self::Float(volume) => volume,
        }
    }
}

#[allow(
    clippy::useless_conversion,
    reason = "QuantityRaw is u64 in standard precision and u128 in high precision"
)]
fn quantity_units(quantity: Quantity, precision: u8) -> u128 {
    u128::from(quantity.raw())
        * 10_u128.pow(u32::from(
            precision - quantity.precision.max(FIXED_PRECISION),
        ))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CompensatedSum {
    value: f64,
    residual: f64,
}

impl From<f64> for CompensatedSum {
    fn from(value: f64) -> Self {
        Self {
            value,
            residual: 0.0,
        }
    }
}

impl CompensatedSum {
    fn from_units(units: u128) -> Self {
        let value = units as f64;
        let rounded = value as u128;
        let error = units.abs_diff(rounded) as f64;
        Self {
            value,
            residual: if units < rounded { -error } else { error },
        }
    }

    fn multiply(self, scale: f64) -> Self {
        let value = self.value * scale;
        Self {
            value,
            residual: self.value.mul_add(scale, -value),
        }
        .add(self.residual * scale)
    }

    fn rescale_to_fixed(self, raw_capacity: f64, capacity: Self, volume_scale: f64) -> Self {
        let previous_capacity = Self::from(raw_capacity);
        if previous_capacity.multiply(volume_scale) == capacity {
            return self.multiply(volume_scale);
        }

        let convert = |distance: Self| {
            let fraction = distance.value / raw_capacity;
            let residual = distance.add_sum(-previous_capacity.multiply(fraction));
            let fraction = Self::from(fraction).add(residual.value / raw_capacity);

            fraction
                .multiply(capacity.value)
                .add_sum(fraction.multiply(capacity.residual))
        };

        // Rescale the smaller distance so rounding cannot erase a nearly full bucket's gap
        let space = previous_capacity.add_sum(-self);
        if self.value <= space.value {
            convert(self)
        } else {
            capacity.add_sum(-convert(space))
        }
    }

    fn add_sum(self, other: Self) -> Self {
        let (sum, residual) = two_sum(self.value, other.value);
        let (value, residual) = two_sum(sum, self.residual + other.residual + residual);
        Self { value, residual }
    }

    fn add(self, value: f64) -> Self {
        self.add_sum(Self::from(value))
    }

    fn is_nonnegative(self) -> bool {
        self.value > 0.0 || (self.value == 0.0 && self.residual >= 0.0)
    }
}

impl std::ops::Neg for CompensatedSum {
    type Output = Self;

    fn neg(self) -> Self {
        Self {
            value: -self.value,
            residual: -self.residual,
        }
    }
}

// Retains the rounding error when a volume is small relative to the bucket
fn two_sum(mut a: f64, mut b: f64) -> (f64, f64) {
    if a.abs() < b.abs() {
        std::mem::swap(&mut a, &mut b);
    }

    let sum = a + b;
    (sum, b - (sum - a))
}

fn bucket_div_rem(volume: f64, bucket: f64) -> (usize, f64) {
    if volume < bucket {
        return (0, volume);
    }

    let difference = volume - bucket;
    if difference < bucket {
        return (1, difference);
    }

    // A floating-point quotient can round across an integer boundary
    let (volume_significand, volume_exponent) = significand_exponent(volume);
    let (bucket_significand, bucket_exponent) = significand_exponent(bucket);
    let shift = volume_exponent - bucket_exponent;
    let count = if shift > volume_significand.leading_zeros() {
        usize::MAX
    } else {
        let count = (volume_significand << shift) / bucket_significand;
        usize::try_from(count).unwrap_or(usize::MAX)
    };

    (count, volume % bucket)
}

fn significand_exponent(value: f64) -> (u128, u32) {
    let bits = value.to_bits();
    let implicit_bit = 1 << (f64::MANTISSA_DIGITS - 1);
    let exponent = (bits / implicit_bit) as u32;
    let mut significand = bits & (implicit_bit - 1);
    if exponent != 0 {
        significand |= implicit_bit;
    }

    (u128::from(significand), exponent.max(1))
}

/// Standard normal CDF, absolute error below 1e-14.
///
/// Hart's (1968) double-precision rational approximation, in the form given by G. West,
/// "Better approximations to cumulative normal functions" (Wilmott, 2005).
fn normal_cdf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let z = x.abs();
    let tail = if z > 37.0 {
        0.0
    } else {
        let e = (-0.5 * z * z).exp();
        if z < 7.071_067_811_865_47 {
            let mut num = 3.526_249_659_989_11e-2_f64.mul_add(z, 0.700_383_064_443_688);
            num = num.mul_add(z, 6.373_962_203_531_65);
            num = num.mul_add(z, 33.912_866_078_383);
            num = num.mul_add(z, 112.079_291_497_871);
            num = num.mul_add(z, 221.213_596_169_931);
            num = num.mul_add(z, 220.206_867_912_376);
            let mut den = 8.838_834_764_831_84e-2_f64.mul_add(z, 1.755_667_163_182_64);
            den = den.mul_add(z, 16.064_177_579_207);
            den = den.mul_add(z, 86.780_732_202_946_1);
            den = den.mul_add(z, 296.564_248_779_674);
            den = den.mul_add(z, 637.333_633_378_831);
            den = den.mul_add(z, 793.826_512_519_948);
            den = den.mul_add(z, 440.413_735_824_752);
            e * num / den
        } else {
            let mut b = z + 0.65;
            b = z + 4.0 / b;
            b = z + 3.0 / b;
            b = z + 2.0 / b;
            b = z + 1.0 / b;
            e / b / 2.506_628_274_631
        }
    };

    if x > 0.0 { 1.0 - tail } else { tail }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::stubs::{bar_ethusdt_binance_minute_bid, stub_quote, stub_trade, vpin_10_2};

    const TOLERANCE: f64 = 1e-12;

    fn feed_sides(indicator: &mut Vpin, trades: &[(AggressorSide, f64)]) {
        for (index, &(side, size)) in trades.iter().enumerate() {
            indicator.update_trade(100.0 + index as f64, size, side);
        }
    }

    #[rstest]
    fn test_name_and_display(vpin_10_2: Vpin) {
        assert_eq!(vpin_10_2.name(), "Vpin");
        assert_eq!(format!("{vpin_10_2}"), "Vpin(10,2,50)");
        assert_eq!(vpin_10_2.bucket_volume(), 10.0);
        assert_eq!(vpin_10_2.window(), 2);
        assert_eq!(vpin_10_2.sigma_window(), 50);
        assert_eq!(vpin_10_2.value, 0.0);
        assert_eq!(vpin_10_2.count, 0);
        assert!(!vpin_10_2.initialized());
        assert!(!vpin_10_2.has_inputs());
    }

    #[rstest]
    #[case(0.0, 2, None)]
    #[case(-1.0, 2, None)]
    #[case(f64::NAN, 2, None)]
    #[case(f64::INFINITY, 2, None)]
    #[case(10.0, 0, None)]
    #[case(10.0, MAX_PERIOD + 1, None)]
    #[case(10.0, 2, Some(1))]
    #[case(10.0, 2, Some(MAX_PERIOD + 1))]
    fn test_invalid_parameters_are_rejected(
        #[case] bucket_volume: f64,
        #[case] window: usize,
        #[case] sigma_window: Option<usize>,
    ) {
        assert!(Vpin::new_checked(bucket_volume, window, sigma_window).is_err());
    }

    #[rstest]
    #[should_panic(expected = "window must be > 0")]
    fn test_zero_window_panics() {
        let _ = Vpin::new(10.0, 0, None);
    }

    #[rstest]
    fn test_aggressor_sides_fill_buckets() {
        // Bucket 1: buy 6 + sell 4 -> |6 - 4| / 10 = 0.2. Bucket 2: sell 10 -> 1.0.
        // VPIN over two buckets = (0.2 + 1.0) / 2 = 0.6.
        let mut indicator = Vpin::new(10.0, 2, None);
        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 6.0), (AggressorSide::Sell, 4.0)],
        );

        assert_eq!(indicator.count, 1);
        assert!(!indicator.initialized());
        assert_eq!(indicator.value, 0.0);

        feed_sides(&mut indicator, &[(AggressorSide::Sell, 10.0)]);

        assert_eq!(indicator.count, 2);
        assert!(indicator.initialized());
        assert!((indicator.value - 0.6).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_alternating_unit_trades_in_odd_buckets() {
        // Ten buys and ten sells of one unit, alternating, into buckets of five units.
        // Every bucket holds three of one side and two of the other: |3 - 2| / 5 = 0.2.
        let mut indicator = Vpin::new(5.0, 4, None);

        for index in 0..20 {
            let side = if index % 2 == 0 {
                AggressorSide::Buy
            } else {
                AggressorSide::Sell
            };
            indicator.update_trade(100.0, 1.0, side);
        }

        assert_eq!(indicator.count, 4);
        assert!((indicator.value - 0.2).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_alternating_unit_trades_in_even_buckets() {
        // Buckets of four units each hold two buys and two sells: VPIN is exactly zero
        let mut indicator = Vpin::new(4.0, 5, None);

        for index in 0..20 {
            let side = if index % 2 == 0 {
                AggressorSide::Buy
            } else {
                AggressorSide::Sell
            };
            indicator.update_trade(100.0, 1.0, side);
        }

        assert_eq!(indicator.count, 5);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_large_trade_spans_buckets() {
        // Buy 25 closes two all-buy buckets (1.0, 1.0) and leaves 5 bought in the third.
        // Sell 15 completes the third bucket at 5 vs 5 (0.0) and fills a fourth with sells (1.0).
        // VPIN over four buckets = (1 + 1 + 0 + 1) / 4 = 0.75.
        let mut indicator = Vpin::new(10.0, 4, None);
        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 25.0), (AggressorSide::Sell, 15.0)],
        );

        assert_eq!(indicator.count, 4);
        assert!((indicator.value - 0.75).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_no_aggressor_uses_tick_rule() {
        // 100 x 4: no previous price, cannot be signed, stays out of the buckets.
        // 101 x 6: uptick, buy 6. 100 x 8: downtick, sell 4 closes bucket 1 (6 vs 4 -> 0.2)
        // and 4 carry over. 100 x 6: zero tick repeats sell, bucket 2 is all sells (1.0).
        let mut indicator = Vpin::new(10.0, 2, None);
        indicator.update_trade(100.0, 4.0, AggressorSide::NoAggressor);

        assert!(indicator.has_inputs());
        assert_eq!(indicator.count, 0);

        indicator.update_trade(101.0, 6.0, AggressorSide::NoAggressor);
        indicator.update_trade(100.0, 8.0, AggressorSide::NoAggressor);
        indicator.update_trade(100.0, 6.0, AggressorSide::NoAggressor);

        assert_eq!(indicator.count, 2);
        assert!((indicator.value - 0.6).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_tick_rule_history_includes_aggressor_trades() {
        // The aggressor trade at 101 still sets the tick history, so the later
        // unsigned trade at 100 is a downtick (sell)
        let mut indicator = Vpin::new(10.0, 1, None);
        indicator.update_trade(101.0, 5.0, AggressorSide::Buy);
        indicator.update_trade(100.0, 5.0, AggressorSide::NoAggressor);

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_bulk_volume_classification() {
        // Closes 100, 101, 100 give changes +1 and -1: sample sigma = sqrt(2).
        // The third bar's buy share is Phi(-1 / sqrt(2)) = erfc(1/2) / 2, so its imbalance is
        // |2 Phi(-1 / sqrt(2)) - 1| = erf(1/2) = 0.520499877813046537682746653892.
        let mut indicator = Vpin::new(10.0, 1, Some(2));
        indicator.update_bar(100.0, 10.0);
        indicator.update_bar(101.0, 10.0);

        assert!(indicator.has_inputs());
        assert_eq!(indicator.count, 0);

        indicator.update_bar(100.0, 10.0);

        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.520_499_877_813_046_5).abs() < 1e-14);
    }

    #[rstest]
    fn test_bulk_volume_classification_flat_window_splits_evenly() {
        let mut indicator = Vpin::new(10.0, 1, Some(2));
        for _ in 0..4 {
            indicator.update_bar(100.0, 10.0);
        }

        assert_eq!(indicator.count, 2);
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_update_raw_split_keeps_buy_share() {
        // 15 buy + 5 sell is a 0.75 buy share: each 10-unit bucket gets 7.5 vs 2.5 -> 0.5
        let mut indicator = Vpin::new(10.0, 2, None);
        indicator.update_raw(15.0, 5.0);

        assert_eq!(indicator.count, 2);
        assert!((indicator.value - 0.5).abs() < TOLERANCE);
    }

    #[rstest]
    #[case(2.0_f64.powi(-42), 1.0 - 2.0_f64.powi(-41), 3.0 * 2.0_f64.powi(-42))]
    #[case(0.5, 0.5_f64.next_down(), 0.5 - 0.5_f64.next_down())]
    fn test_raw_volume_carries_until_bucket_is_full(
        #[case] buy: f64,
        #[case] sell: f64,
        #[case] remaining: f64,
    ) {
        let mut indicator = Vpin::new(1.0, 1, None);
        indicator.update_raw(buy, 0.0);

        assert!(indicator.has_inputs());

        indicator.update_raw(0.0, sell);

        assert_eq!(indicator.count, 0);
        assert!(!indicator.initialized());

        indicator.update_raw(0.0, remaining);

        assert_eq!(indicator.count, 1);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 1.0 - 2.0 * buy);
    }

    #[rstest]
    fn test_raw_volume_spanning_large_bucket_count_preserves_remainder() {
        let mut indicator = Vpin::new(3.0, 2, None);
        let volume = 1_u64 << 60;
        let buckets = usize::try_from(volume / 3).unwrap_or(usize::MAX);
        indicator.update_raw(0.0, 1.0);
        indicator.update_raw(volume as f64, 0.0);

        // The original sell and the print's one-unit remainder leave a two-unit partial
        assert_eq!(indicator.count, buckets);
        assert_eq!(indicator.value, 1.0);

        indicator.update_raw(0.0, 1.0);

        assert_eq!(indicator.count, buckets.saturating_add(1));
        assert!((indicator.value - 2.0 / 3.0).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_raw_volume_accepts_finite_sides_whose_sum_overflows() {
        let mut indicator = Vpin::new(f64::MAX, 2, None);
        indicator.update_raw(f64::MAX, f64::MAX);

        assert_eq!(indicator.count, 2);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_raw_volume_fills_large_bucket_with_existing_partial() {
        let mut indicator = Vpin::new(f64::MAX, 2, None);
        let partial = 3.0 * 2.0_f64.powi(970);
        indicator.update_raw(partial, 0.0);
        indicator.update_raw(f64::MAX, f64::MAX);

        assert_eq!(indicator.count, 2);
        assert_eq!(indicator.value, (partial / f64::MAX) / 2.0);
    }

    #[rstest]
    fn test_raw_halves_fill_fractional_bucket() {
        let mut indicator = Vpin::new(0.3, 1, None);
        indicator.update_raw(0.15, 0.0);
        indicator.update_raw(0.0, 0.15);

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_raw_volume_keeps_rolling_after_count_saturates() {
        let mut indicator = Vpin::new(1.0, 2, None);
        indicator.update_raw(f64::MAX, 0.0);

        assert_eq!(indicator.count, usize::MAX);
        assert_eq!(indicator.value, 1.0);

        indicator.update_raw(1.0, 1.0);

        assert_eq!(indicator.count, usize::MAX);
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    #[case(f64::NAN, 1.0)]
    #[case(1.0, f64::INFINITY)]
    #[case(-1.0, 2.0)]
    #[case(0.0, 0.0)]
    fn test_rejected_raw_input_leaves_state_unchanged(#[case] buy: f64, #[case] sell: f64) {
        let mut indicator = Vpin::new(10.0, 1, None);
        indicator.update_raw(buy, sell);

        assert!(!indicator.has_inputs());
        assert_eq!(indicator.count, 0);
    }

    #[rstest]
    #[case(f64::NAN, 1.0)]
    #[case(100.0, 0.0)]
    #[case(100.0, -1.0)]
    #[case(100.0, f64::NAN)]
    fn test_rejected_trade_leaves_tick_history_unchanged(#[case] price: f64, #[case] size: f64) {
        let mut indicator = Vpin::new(10.0, 1, None);
        indicator.update_trade(price, size, AggressorSide::NoAggressor);

        assert!(!indicator.has_inputs());

        // Had the rejected trade set a previous price, this trade would be signed
        indicator.update_trade(99.0, 10.0, AggressorSide::NoAggressor);
        assert_eq!(indicator.count, 0);
    }

    #[rstest]
    fn test_handle_trade_uses_aggressor_side(stub_trade: TradeTick) {
        let mut indicator = Vpin::new(1.0, 1, None);
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    fn test_handle_trade_preserves_fixed_point_tick_direction(mut stub_trade: TradeTick) {
        let mut indicator = Vpin::new(stub_trade.size.as_f64(), 1, None);
        let price = Price::from("100000000");
        stub_trade.aggressor_side = AggressorSide::NoAggressor;
        stub_trade.price = price;
        indicator.handle_trade(&stub_trade);

        stub_trade.price = Price::from_raw(price.raw() + 1, FIXED_PRECISION);
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    #[case(0.1, "0.1")]
    #[case(100.01, "100.01")]
    #[case(64_250.7, "64250.7")]
    #[case(59_031.0, "59031")]
    #[case(0.900_741_577_148_437_5, "0.9007415771484375")]
    fn test_mixed_trade_prices_do_not_create_a_tick(
        #[case] raw: f64,
        #[case] model: &str,
        mut stub_trade: TradeTick,
    ) {
        let mut indicator = Vpin::new(stub_trade.size.as_f64(), 1, None);
        let (raw, price) = model
            .parse()
            .map_or_else(|_| (0.5, Price::from("0.5")), |price| (raw, price));
        indicator.update_trade(raw, stub_trade.size.as_f64(), AggressorSide::NoAggressor);
        stub_trade.aggressor_side = AggressorSide::NoAggressor;
        stub_trade.price = price;
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 0);
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn test_mixed_trade_prices_preserve_sub_ulp_tick(
        #[case] model_last: bool,
        mut stub_trade: TradeTick,
    ) {
        let mut indicator = Vpin::new(2.0, 1, None);
        let base = Price::from("100000000");
        stub_trade.price = Price::from_raw(base.raw() + 1, FIXED_PRECISION);
        stub_trade.size = Quantity::from("1");
        stub_trade.aggressor_side = AggressorSide::NoAggressor;

        if model_last {
            indicator.update_raw(0.0, 1.0);
            indicator.update_trade(100_000_000.0, 1.0, AggressorSide::NoAggressor);
            indicator.handle_trade(&stub_trade);
        } else {
            indicator.update_raw(1.0, 0.0);
            indicator.handle_trade(&stub_trade);
            indicator.update_trade(100_000_000.0, 1.0, AggressorSide::NoAggressor);
        }

        assert_eq!(indicator.count, 1);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_handle_trade_decimal_sizes_fill_bucket(mut stub_trade: TradeTick) {
        let mut indicator = Vpin::new(1.0, 1, None);
        stub_trade.aggressor_side = AggressorSide::Buy;
        stub_trade.size = Quantity::from("0.3");
        indicator.handle_trade(&stub_trade);

        stub_trade.aggressor_side = AggressorSide::Sell;
        stub_trade.size = Quantity::from("0.7");
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.4).abs() < TOLERANCE);
    }

    #[rstest]
    #[case(300_000.7, &["100000.2", "100000.2", "100000.3"])]
    #[case(2_500.1, &["2323.40473214", "176.69526786"])]
    #[case(18_234.37, &["13697.0313", "4537.3387"])]
    fn test_large_decimal_buckets_fill_with_model_quantities(
        #[case] bucket: f64,
        #[case] quantities: &[&str],
        mut stub_trade: TradeTick,
    ) {
        let mut indicator = Vpin::new(bucket, 1, None);
        stub_trade.aggressor_side = AggressorSide::Buy;
        for size in quantities {
            stub_trade.size = Quantity::from(*size);
            indicator.handle_trade(&stub_trade);
        }

        assert_eq!(indicator.count, 1);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    fn test_handle_trade_preserves_native_quantity_precision(mut stub_trade: TradeTick) {
        let (bucket, buy, sell) = if let Ok(buy) = "0.000000000000000007".parse::<Quantity>() {
            (1e-16, buy, Quantity::from("0.000000000000000093"))
        } else {
            (1.0, Quantity::from("0.07"), Quantity::from("0.93"))
        };

        let mut indicator = Vpin::new(bucket, 1, None);
        stub_trade.aggressor_side = AggressorSide::Buy;
        stub_trade.size = buy;
        indicator.handle_trade(&stub_trade);

        stub_trade.aggressor_side = AggressorSide::Sell;
        stub_trade.size = sell;
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.86).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_handle_trade_accepts_supported_bucket_precision(mut stub_trade: TradeTick) {
        let (bucket, size) = if let Ok(size) = "0.30000000000000021".parse::<Quantity>() {
            (0.10000000000000007, size)
        } else {
            (0.1, Quantity::from("0.3"))
        };

        let mut indicator = Vpin::new(bucket, 1, None);
        stub_trade.aggressor_side = AggressorSide::Buy;
        stub_trade.size = size;
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 3);
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    #[case(0.3, "0.15", 0.15, 0.0)]
    #[case(11.0, "6", 5.0, 1.0 / 11.0)]
    #[case(59_031.0, "29515.5", 29_515.5, 0.0)]
    #[case(59_037.0, "19679", 39_358.0, 1.0 / 3.0)]
    fn test_mixed_volume_inputs_fill_bucket(
        #[case] bucket: f64,
        #[case] model: &str,
        #[case] raw: f64,
        #[case] expected: f64,
        #[values(true, false)] model_first: bool,
        mut stub_trade: TradeTick,
    ) {
        let mut indicator = Vpin::new(bucket, 1, None);
        stub_trade.aggressor_side = AggressorSide::Sell;
        stub_trade.size = Quantity::from(model);
        if model_first {
            indicator.handle_trade(&stub_trade);
            indicator.update_raw(raw, 0.0);
        } else {
            indicator.update_raw(raw, 0.0);
            indicator.handle_trade(&stub_trade);
        }

        assert_eq!(indicator.count, 1);
        assert!((indicator.value - expected).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_alternating_volume_sources_complete_both_buckets(mut stub_trade: TradeTick) {
        let mut indicator = Vpin::new(1.0, 2, None);
        stub_trade.aggressor_side = AggressorSide::Buy;
        stub_trade.size = Quantity::from("0.68");
        indicator.handle_trade(&stub_trade);
        indicator.update_raw(0.375, 0.0);

        stub_trade.aggressor_side = AggressorSide::Sell;
        stub_trade.size = Quantity::from("0.945");
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 2);
        assert!(indicator.initialized());
        assert!((indicator.value - 0.945).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_raw_volume_between_model_trades_preserves_exact_fill(mut stub_trade: TradeTick) {
        let mut indicator = Vpin::new(1.0, 1, None);
        stub_trade.aggressor_side = AggressorSide::Buy;
        stub_trade.size = Quantity::from("0.05");
        indicator.handle_trade(&stub_trade);
        indicator.update_raw(0.5, 0.0);
        stub_trade.size = Quantity::from("0.45");
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 1);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    fn test_handle_bar_uses_close_and_volume() {
        let mut indicator = Vpin::new(100_000.0, 1, Some(2));
        for close in ["1500", "1510", "1500"] {
            indicator.handle_bar(&bar_ethusdt_binance_minute_bid(close));
        }

        // Same changes (+10, -10) as the bulk classification test, scaled: imbalance erf(1/2)
        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.520_499_877_813_046_5).abs() < 1e-14);
    }

    #[rstest]
    #[case(2, &["100", "101", "102"])]
    #[case(2, &["100.00", "100.01", "100.02"])]
    #[case(4, &["100.00", "100.03", "100.07", "100.08", "100.09", "100.10", "100.11"])]
    #[case(5, &[
        "100000.00", "105756.09", "105700.90", "105700.75", "105700.73",
        "105700.72", "105700.71", "105700.70", "105700.69", "105700.68",
    ])]
    fn test_handle_bar_equal_close_changes_split_evenly(
        #[case] sigma_window: usize,
        #[case] closes: &[&str],
    ) {
        let mut indicator = Vpin::new(100_000.0, 1, Some(sigma_window));
        for &close in &closes[..closes.len() - 1] {
            indicator.handle_bar(&bar_ethusdt_binance_minute_bid(close));
        }

        let earlier_buckets = closes.len() - sigma_window - 1;
        assert_eq!(indicator.count, earlier_buckets);

        indicator.handle_bar(&bar_ethusdt_binance_minute_bid(closes[closes.len() - 1]));

        assert_eq!(indicator.count, earlier_buckets + 1);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    #[case(4, &["100", "99.61", "99.852", "99.852", "99.852", "99.852", "99.852", "99.852000001"])]
    #[case(3, &["0", "0", "1", "1.000000001", "1.000000001", "1.000000003"])]
    fn test_handle_bar_reseeds_moments_after_large_eviction(
        #[case] sigma_window: usize,
        #[case] closes: &[&str],
    ) {
        let mut indicator = Vpin::new(100_000.0, 1, Some(sigma_window));
        for close in closes {
            indicator.handle_bar(&bar_ethusdt_binance_minute_bid(close));
        }

        // In both windows the latest change divided by sample sigma is 2
        assert_eq!(indicator.count, closes.len() - sigma_window);
        assert!((indicator.value - 0.954_499_736_103_641_6).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_handle_bar_retains_sub_ulp_differences_between_close_changes() {
        let mut indicator = Vpin::new(100_000.0, 1, Some(2));
        let mut bar = bar_ethusdt_binance_minute_bid("0");
        indicator.handle_bar(&bar);

        bar.close = Price::from("100000000");
        indicator.handle_bar(&bar);

        // The second change exceeds the first by a single fixed-point unit
        bar.close = Price::from_raw(2 * bar.close.raw() + 1, FIXED_PRECISION);
        indicator.handle_bar(&bar);

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    fn test_mixed_bar_prices_keep_digits_below_display_precision() {
        let mut indicator = Vpin::new(100_000.0, 1, Some(2));
        let mut bar = bar_ethusdt_binance_minute_bid("100.005");
        bar.close.precision = 2;

        indicator.update_bar(100.0, 100_000.0);
        indicator.handle_bar(&bar);
        indicator.update_bar(100.0, 100_000.0);

        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.520_499_877_813_046_5).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_mixed_bar_prices_preserve_sub_ulp_close_changes() {
        let mut indicator = Vpin::new(100_000.0, 1, Some(2));
        let mut bar = bar_ethusdt_binance_minute_bid("100000000");
        bar.close = Price::from_raw(bar.close.raw() + 1, FIXED_PRECISION);

        indicator.update_bar(100_000_000.0, 100_000.0);
        indicator.handle_bar(&bar);
        indicator.update_bar(100_000_000.0, 100_000.0);

        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.520_499_877_813_046_5).abs() < TOLERANCE);
    }

    #[rstest]
    #[case("100", "101.000000000000000000", "102")]
    #[case("1", "2364", "4727.000000000000000000")]
    fn test_handle_bar_aligns_supported_price_scales(
        #[case] first: &str,
        #[case] second: &str,
        #[case] third: &str,
    ) {
        let mut indicator = Vpin::new(100_000.0, 1, Some(2));

        for close in [first, second, third] {
            let mut bar = bar_ethusdt_binance_minute_bid("100");
            bar.close = close
                .parse()
                .unwrap_or_else(|_| Price::from(close.trim_end_matches('0').trim_end_matches('.')));
            indicator.handle_bar(&bar);
        }

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_quote_is_ignored(stub_quote: QuoteTick) {
        let mut indicator = Vpin::new(10.0, 1, None);
        let result = indicator.handle_quote(&stub_quote);

        assert!(result.is_ok());
        assert!(!indicator.has_inputs());
    }

    #[rstest]
    fn test_reset_restores_fresh_state() {
        let mut indicator = Vpin::new(10.0, 2, Some(2));
        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 6.0), (AggressorSide::Sell, 14.0)],
        );
        indicator.update_bar(100.0, 3.0);
        indicator.reset();

        assert_eq!(indicator.value, 0.0);
        assert_eq!(indicator.count, 0);
        assert!(!indicator.initialized());
        assert!(!indicator.has_inputs());

        // The first unsigned trade after a reset has no tick history
        indicator.update_trade(100.0, 10.0, AggressorSide::NoAggressor);
        assert_eq!(indicator.count, 0);

        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 6.0), (AggressorSide::Sell, 14.0)],
        );
        assert_eq!(indicator.count, 2);
        assert!((indicator.value - 0.6).abs() < TOLERANCE);
    }

    #[rstest]
    #[case(0.0, 0.5)]
    #[case(1.0, 0.841_344_746_068_542_9)]
    #[case(-1.959_963_984_540_054, 0.025)]
    #[case(-8.0, 6.220_960_574_271_784e-16)]
    fn test_normal_cdf_reference_values(#[case] x: f64, #[case] expected: f64) {
        assert!((normal_cdf(x) - expected).abs() < 1e-14);
    }

    proptest! {
        #[rstest]
        fn prop_value_is_invariant_to_splitting_trades(
            bucket_units in 1_u32..=8,
            window in 1_usize..=6,
            trades in prop::collection::vec((any::<bool>(), 1_u32..=12), 1..=64),
        ) {
            // Integer sizes keep every bucket boundary exact, so the split stream
            // must give the same value as the whole trades
            let mut whole = Vpin::new(f64::from(bucket_units), window, None);
            let mut split = Vpin::new(f64::from(bucket_units), window, None);

            for &(is_buy, size) in &trades {
                let side = if is_buy { AggressorSide::Buy } else { AggressorSide::Sell };
                whole.update_trade(100.0, f64::from(size), side);
                for _ in 0..size {
                    split.update_trade(100.0, 1.0, side);
                }
            }

            prop_assert_eq!(whole.count, split.count);
            prop_assert_eq!(whole.initialized, split.initialized);
            prop_assert!((whole.value - split.value).abs() < TOLERANCE);
            prop_assert!((0.0..=1.0).contains(&whole.value));
        }
    }
}
