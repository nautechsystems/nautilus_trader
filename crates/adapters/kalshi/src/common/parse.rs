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

//! Kalshi instrument identity and exact price grids.

use anyhow::Context;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::tick_scheme::{
        TickScheme, TieredTickScheme, get_tick_scheme, register_tick_scheme,
    },
    types::Price,
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use ustr::Ustr;

use crate::{
    common::consts::{KALSHI_VENUE, PRICE_PRECISION},
    http::models::PriceRange,
};

/// Converts a native, case-sensitive ticker to its canonical instrument ID.
///
/// Accepts ASCII letters, digits, hyphens, and decimal points between digits.
///
/// # Errors
///
/// Returns an error if the ticker is empty or contains unsupported characters.
pub fn parse_instrument_id(ticker: &str) -> anyhow::Result<InstrumentId> {
    validate_ticker(ticker)?;
    Ok(InstrumentId::new(Symbol::new(ticker), *KALSHI_VENUE))
}

/// Returns a canonical instrument's native ticker without changing its case.
///
/// # Errors
///
/// Returns an error if the venue is not KALSHI or the market ticker is invalid.
pub fn parse_ticker(instrument_id: InstrumentId) -> anyhow::Result<String> {
    anyhow::ensure!(
        instrument_id.venue == *KALSHI_VENUE,
        "instrument venue must be KALSHI"
    );
    let ticker = instrument_id.symbol.as_str();
    validate_ticker(ticker)?;
    Ok(ticker.to_owned())
}

/// Exact market bands used for admission and explicit price validation.
///
/// Native limit submission enforces the lattice through `contains`; simulation does not.
///
/// Storage uses six decimal places independently of the current venue grid.
#[derive(Debug, Clone)]
pub struct KalshiPriceGrid {
    ranges: Vec<PriceRange>,
    increment: Price,
}

impl KalshiPriceGrid {
    /// Validates ordered, non-overlapping bands within the binary payout interval.
    ///
    /// # Errors
    ///
    /// Returns an error if bands are missing, bounds or steps are invalid, or precision exceeds six places.
    pub fn new(ranges: &[PriceRange]) -> anyhow::Result<Self> {
        anyhow::ensure!(!ranges.is_empty(), "missing price ranges");
        let mut quantum = 0;
        let mut previous_end = Decimal::ZERO;

        for range in ranges {
            let start = range.start.0;
            let end = range.end.0;
            let step = range.step.0;
            anyhow::ensure!(
                start >= previous_end
                    && start >= Decimal::ZERO
                    && start < end
                    && end <= Decimal::ONE,
                "invalid or overlapping price range"
            );
            anyhow::ensure!(
                step > Decimal::ZERO && step < end - start,
                "invalid price range step"
            );

            for value in [start, end, step] {
                anyhow::ensure!(
                    value.normalize().scale() <= u32::from(PRICE_PRECISION),
                    "price range exceeds six decimal places"
                );
            }

            for value in [start, step] {
                let scaled = (value * Decimal::from(10_i64.pow(u32::from(PRICE_PRECISION))))
                    .to_i64()
                    .context("invalid price quantum")?;
                quantum = gcd(quantum, scaled);
            }

            previous_end = end;
        }

        let increment = Price::from_decimal_dp(
            Decimal::new(quantum, u32::from(PRICE_PRECISION)),
            PRICE_PRECISION,
        )?;
        Ok(Self {
            ranges: ranges.to_vec(),
            increment,
        })
    }

    /// Returns the base quantum; full band validation remains required.
    #[must_use]
    pub const fn increment(&self) -> Price {
        self.increment
    }

    /// Tests a strict interior binary price against its exact band lattice.
    #[must_use]
    pub fn contains(&self, price: Decimal) -> bool {
        price > Decimal::ZERO
            && price < Decimal::ONE
            && self.ranges.iter().any(|range| {
                price >= range.start.0
                    && price < range.end.0
                    && (price - range.start.0) % range.step.0 == Decimal::ZERO
            })
    }

    /// Registers or reuses a content-named navigation scheme after exact tick verification.
    ///
    /// Call this again after loading a persisted instrument's bands on restart.
    ///
    /// # Errors
    ///
    /// Returns an error if the shared scheme cannot reproduce the decimal lattice exactly.
    pub fn register(&self) -> anyhow::Result<Ustr> {
        let mut name = String::from("KALSHI");
        let mut tiers = Vec::with_capacity(self.ranges.len());
        let mut expected = Vec::new();

        for range in &self.ranges {
            let [start, end, step] = [range.start.0, range.end.0, range.step.0];
            name.push_str(&format!(
                "_{}_{}_{}",
                start.normalize(),
                end.normalize(),
                step.normalize()
            ));

            let start = if start.is_zero() { step } else { start };
            // Exclude a rounded-up end tick from the shared floating-point constructor
            tiers.push((
                start.to_f64().context("invalid band start")?,
                (end - Decimal::new(5, u32::from(PRICE_PRECISION) + 1))
                    .to_f64()
                    .context("invalid band end")?,
                step.to_f64().context("invalid band step")?,
            ));
            let mut value = start;
            while value < end {
                expected.push(Price::from_decimal_dp(value, PRICE_PRECISION)?);
                value += step;
            }
        }

        let scheme = TieredTickScheme::new(
            &tiers,
            PRICE_PRECISION,
            10_usize.pow(u32::from(PRICE_PRECISION)) + 1,
        )?;

        anyhow::ensure!(
            scheme.ticks() == expected,
            "navigation scheme differs from exact price grid"
        );
        let scheme = TickScheme::Tiered(scheme);
        if let Err(e) = register_tick_scheme(&name, scheme.clone()) {
            anyhow::ensure!(
                get_tick_scheme(&name) == Some(scheme),
                "inconsistent registered price grid: {e}"
            );
        }

        Ok(Ustr::from(&name))
    }
}

fn validate_ticker(ticker: &str) -> anyhow::Result<()> {
    let bytes = ticker.as_bytes();
    anyhow::ensure!(
        !ticker.is_empty()
            && bytes.iter().enumerate().all(|(index, c)| {
                c.is_ascii_alphanumeric()
                    || *c == b'-'
                    || (*c == b'.'
                        && index > 0
                        && bytes[index - 1].is_ascii_digit()
                        && bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
            }),
        "invalid Kalshi ticker"
    );
    Ok(())
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }

    a
}
