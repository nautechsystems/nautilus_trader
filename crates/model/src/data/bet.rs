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

//! Domain model representing a *Bet* used by betting-market integrations (e.g. prediction markets).

use std::fmt::Display;

use rust_decimal::Decimal;

use crate::enums::{BetSide, OrderSide, OrderSideSpecified};

/// A bet in a betting market.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct Bet {
    price: Decimal,
    stake: Decimal,
    side: BetSide,
}

impl Bet {
    /// Creates a new [`Bet`] instance.
    #[must_use]
    pub fn new(price: Decimal, stake: Decimal, side: BetSide) -> Self {
        Self { price, stake, side }
    }

    /// Returns the bet's price.
    #[must_use]
    pub fn price(&self) -> Decimal {
        self.price
    }

    /// Returns the bet's stake.
    #[must_use]
    pub fn stake(&self) -> Decimal {
        self.stake
    }

    /// Returns the bet's side.
    #[must_use]
    pub fn side(&self) -> BetSide {
        self.side
    }

    /// Creates a bet from a stake or liability depending on the bet side.
    ///
    /// For `BetSide::Back` this calls [`Self::from_stake`] and for
    /// `BetSide::Lay` it calls [`Self::from_liability`].
    ///
    /// # Panics
    ///
    /// Panics if `side` is [`BetSide::Lay`] and [`Self::from_liability`] panics.
    #[must_use]
    pub fn from_stake_or_liability(price: Decimal, volume: Decimal, side: BetSide) -> Self {
        Self::from_stake_or_liability_checked(price, volume, side).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Creates a bet from a stake or liability depending on the bet side.
    ///
    /// # Errors
    ///
    /// Returns an error if `side` is [`BetSide::Lay`] and [`Self::from_liability_checked`] fails.
    pub fn from_stake_or_liability_checked(
        price: Decimal,
        volume: Decimal,
        side: BetSide,
    ) -> anyhow::Result<Self> {
        match side {
            BetSide::Back => Ok(Self::from_stake(price, volume, side)),
            BetSide::Lay => Self::from_liability_checked(price, volume, side),
        }
    }

    /// Creates a bet from a given stake.
    #[must_use]
    pub fn from_stake(price: Decimal, stake: Decimal, side: BetSide) -> Self {
        Self::new(price, stake, side)
    }

    /// Creates a bet from a given liability.
    ///
    /// # Panics
    ///
    /// Panics if the side is not [`BetSide::Lay`], if `price` is not greater than 1,
    /// or if the stake calculation overflows.
    #[must_use]
    pub fn from_liability(price: Decimal, liability: Decimal, side: BetSide) -> Self {
        Self::from_liability_checked(price, liability, side).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Creates a bet from a given liability.
    ///
    /// # Errors
    ///
    /// Returns an error if the side is not [`BetSide::Lay`], if `price` is not greater
    /// than 1, or if the stake calculation overflows.
    pub fn from_liability_checked(
        price: Decimal,
        liability: Decimal,
        side: BetSide,
    ) -> anyhow::Result<Self> {
        if side != BetSide::Lay {
            anyhow::bail!("Liability-based betting is only applicable for Lay side.");
        }

        check_odds_gt_one(price)?;
        let stake = checked_div(liability, checked_sub(price, Decimal::ONE)?)?;
        Ok(Self::new(price, stake, side))
    }

    /// Returns the bet's exposure.
    ///
    /// For BACK bets, exposure is positive; for LAY bets, it is negative.
    ///
    /// # Panics
    ///
    /// Panics if the calculation overflows.
    #[must_use]
    pub fn exposure(&self) -> Decimal {
        self.exposure_checked().unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the bet's exposure.
    ///
    /// # Errors
    ///
    /// Returns an error if the calculation overflows.
    pub fn exposure_checked(&self) -> anyhow::Result<Decimal> {
        let notional = checked_mul(self.price, self.stake)?;
        Ok(match self.side {
            BetSide::Back => notional,
            BetSide::Lay => -notional,
        })
    }

    /// Returns the bet's liability.
    ///
    /// For BACK bets, liability equals the stake; for LAY bets, it is
    /// stake multiplied by (price - 1).
    ///
    /// # Panics
    ///
    /// Panics if the calculation overflows.
    #[must_use]
    pub fn liability(&self) -> Decimal {
        self.liability_checked().unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the bet's liability.
    ///
    /// # Errors
    ///
    /// Returns an error if the calculation overflows.
    pub fn liability_checked(&self) -> anyhow::Result<Decimal> {
        match self.side {
            BetSide::Back => Ok(self.stake),
            BetSide::Lay => checked_mul(self.stake, checked_sub(self.price, Decimal::ONE)?),
        }
    }

    /// Returns the bet's profit.
    ///
    /// For BACK bets, profit is stake * (price - 1); for LAY bets it equals the stake.
    ///
    /// # Panics
    ///
    /// Panics if the calculation overflows.
    #[must_use]
    pub fn profit(&self) -> Decimal {
        self.profit_checked().unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the bet's profit.
    ///
    /// # Errors
    ///
    /// Returns an error if the calculation overflows.
    pub fn profit_checked(&self) -> anyhow::Result<Decimal> {
        match self.side {
            BetSide::Back => checked_mul(self.stake, checked_sub(self.price, Decimal::ONE)?),
            BetSide::Lay => Ok(self.stake),
        }
    }

    /// Returns the outcome win payoff.
    ///
    /// For BACK bets this is the profit; for LAY bets it is the negative liability.
    ///
    /// # Panics
    ///
    /// Panics if the calculation overflows.
    #[must_use]
    pub fn outcome_win_payoff(&self) -> Decimal {
        self.outcome_win_payoff_checked()
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the outcome win payoff.
    ///
    /// # Errors
    ///
    /// Returns an error if the calculation overflows.
    pub fn outcome_win_payoff_checked(&self) -> anyhow::Result<Decimal> {
        match self.side {
            BetSide::Back => self.profit_checked(),
            BetSide::Lay => Ok(-self.liability_checked()?),
        }
    }

    /// Returns the outcome lose payoff.
    ///
    /// For BACK bets this is the negative liability; for LAY bets it is the profit.
    ///
    /// # Panics
    ///
    /// Panics if the calculation overflows.
    #[must_use]
    pub fn outcome_lose_payoff(&self) -> Decimal {
        self.outcome_lose_payoff_checked()
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the outcome lose payoff.
    ///
    /// # Errors
    ///
    /// Returns an error if the calculation overflows.
    pub fn outcome_lose_payoff_checked(&self) -> anyhow::Result<Decimal> {
        match self.side {
            BetSide::Back => Ok(-self.liability_checked()?),
            BetSide::Lay => self.profit_checked(),
        }
    }

    /// Returns the hedging stake given a new price.
    ///
    /// # Panics
    ///
    /// Panics if `price` is zero, if this bet's price is zero on the lay path,
    /// or if the calculation overflows.
    #[must_use]
    pub fn hedging_stake(&self, price: Decimal) -> Decimal {
        self.hedging_stake_checked(price)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the hedging stake given a new price.
    ///
    /// # Errors
    ///
    /// Returns an error if `price` is zero, if this bet's price is zero on the
    /// lay path, or if the calculation overflows.
    pub fn hedging_stake_checked(&self, price: Decimal) -> anyhow::Result<Decimal> {
        match self.side {
            BetSide::Back => checked_mul(checked_div(self.price, price)?, self.stake),
            BetSide::Lay => checked_div(self.stake, checked_div(price, self.price)?),
        }
    }

    /// Creates a hedging bet for a given price.
    ///
    /// # Panics
    ///
    /// Panics if [`Self::hedging_stake`] panics.
    #[must_use]
    pub fn hedging_bet(&self, price: Decimal) -> Self {
        self.hedging_bet_checked(price)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Creates a hedging bet for a given price.
    ///
    /// # Errors
    ///
    /// Returns an error if [`Self::hedging_stake_checked`] fails.
    pub fn hedging_bet_checked(&self, price: Decimal) -> anyhow::Result<Self> {
        Ok(Self::new(
            price,
            self.hedging_stake_checked(price)?,
            self.side.opposite(),
        ))
    }
}

impl Display for Bet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Example output: "Bet(Back @ 2.50 x10.00)"
        write!(
            f,
            "Bet({:?} @ {:.2} x{:.2})",
            self.side, self.price, self.stake
        )
    }
}

/// A position comprising one or more bets.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BetPosition {
    price: Decimal,
    exposure: Decimal,
    realized_pnl: Decimal,
    bets: Vec<Bet>,
}

impl Default for BetPosition {
    fn default() -> Self {
        Self {
            price: Decimal::ZERO,
            exposure: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            bets: vec![],
        }
    }
}

impl BetPosition {
    /// Returns the position's price.
    #[must_use]
    pub fn price(&self) -> Decimal {
        self.price
    }

    /// Returns the position's exposure.
    #[must_use]
    pub fn exposure(&self) -> Decimal {
        self.exposure
    }

    /// Returns the position's realized profit and loss.
    #[must_use]
    pub fn realized_pnl(&self) -> Decimal {
        self.realized_pnl
    }

    /// Returns a reference to the position's bets.
    #[must_use]
    pub fn bets(&self) -> &[Bet] {
        &self.bets
    }

    /// Returns the overall side of the position.
    ///
    /// If exposure is positive the side is BACK; if negative, LAY; if zero, None.
    #[must_use]
    pub fn side(&self) -> Option<BetSide> {
        match self.exposure.cmp(&Decimal::ZERO) {
            std::cmp::Ordering::Less => Some(BetSide::Lay),
            std::cmp::Ordering::Greater => Some(BetSide::Back),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// Converts the current position into a single bet, if possible.
    ///
    /// # Panics
    ///
    /// Panics if the position has a side and `price` is zero, or if the
    /// calculation overflows.
    #[must_use]
    pub fn as_bet(&self) -> Option<Bet> {
        self.as_bet_checked().unwrap_or_else(|e| panic!("{e}"))
    }

    /// Converts the current position into a single bet, if possible.
    ///
    /// # Errors
    ///
    /// Returns an error if the position has a side and `price` is zero, or if
    /// the calculation overflows.
    pub fn as_bet_checked(&self) -> anyhow::Result<Option<Bet>> {
        let Some(side) = self.side() else {
            return Ok(None);
        };
        check_nonzero_denominator(self.price, "price")?;
        let stake = match side {
            BetSide::Back => checked_div(self.exposure, self.price)?,
            BetSide::Lay => checked_div(-self.exposure, self.price)?,
        };
        Ok(Some(Bet::new(self.price, stake, side)))
    }

    /// Adds a bet to the position, adjusting exposure and realized PnL.
    pub fn add_bet(&mut self, bet: Bet) {
        match self.side() {
            None => self.position_increase(&bet),
            Some(current_side) => {
                if current_side == bet.side {
                    self.position_increase(&bet);
                } else {
                    self.position_decrease(&bet);
                }
            }
        }
        self.bets.push(bet);
    }

    /// Adds a bet to the position, adjusting exposure and realized PnL.
    ///
    /// # Errors
    ///
    /// Returns an error if a denominator is zero or a Decimal calculation overflows.
    /// On error the position is left unchanged.
    pub fn add_bet_checked(&mut self, bet: Bet) -> anyhow::Result<()> {
        let (price, exposure, realized_pnl) = match self.side() {
            None => self.increased_state(&bet)?,
            Some(current_side) if current_side == bet.side => self.increased_state(&bet)?,
            Some(_) => self.decreased_state(&bet)?,
        };
        self.price = price;
        self.exposure = exposure;
        self.realized_pnl = realized_pnl;
        self.bets.push(bet);
        Ok(())
    }

    fn increased_state(&self, bet: &Bet) -> anyhow::Result<(Decimal, Decimal, Decimal)> {
        let bet_exposure = bet.exposure_checked()?;
        let price = if self.side().is_none() {
            bet.price
        } else if self.side() == Some(bet.side)
            && self.price > Decimal::ZERO
            && bet.price > Decimal::ZERO
            && bet.stake > Decimal::ZERO
        {
            let abs_self_exposure = self.exposure.abs();
            let abs_bet_exposure = bet_exposure.abs();
            let total_stake = checked_add(checked_div(abs_self_exposure, self.price)?, bet.stake)?;
            checked_div(
                checked_add(abs_self_exposure, abs_bet_exposure)?,
                total_stake,
            )?
        } else {
            self.price
        };
        Ok((
            price,
            checked_add(self.exposure, bet_exposure)?,
            self.realized_pnl,
        ))
    }

    fn decreased_state(&self, bet: &Bet) -> anyhow::Result<(Decimal, Decimal, Decimal)> {
        let current_side = self
            .side()
            .ok_or_else(|| anyhow::anyhow!("cannot decrease an empty bet position"))?;
        let bet_exposure = bet.exposure_checked()?;
        let abs_bet_exposure = bet_exposure.abs();
        let abs_self_exposure = self.exposure.abs();

        match abs_bet_exposure.cmp(&abs_self_exposure) {
            std::cmp::Ordering::Less => {
                check_nonzero_denominator(self.price, "price")?;
                let decreasing_volume = checked_div(abs_bet_exposure, self.price)?;
                let decreasing_bet = Bet::new(self.price, decreasing_volume, current_side);
                let pnl = calc_bets_pnl_checked(&[bet.clone(), decreasing_bet])?;
                Ok((
                    self.price,
                    checked_add(self.exposure, bet_exposure)?,
                    checked_add(self.realized_pnl, pnl)?,
                ))
            }
            std::cmp::Ordering::Greater => Ok((
                bet.price,
                checked_add(self.exposure, bet_exposure)?,
                self.realized_after_close(bet)?,
            )),
            std::cmp::Ordering::Equal => Ok((
                Decimal::ZERO,
                Decimal::ZERO,
                self.realized_after_close(bet)?,
            )),
        }
    }

    fn realized_after_close(&self, bet: &Bet) -> anyhow::Result<Decimal> {
        match self.as_bet_checked()? {
            Some(self_bet) => checked_add(
                self.realized_pnl,
                calc_bets_pnl_checked(&[bet.clone(), self_bet])?,
            ),
            None => Ok(self.realized_pnl),
        }
    }

    /// Increases the position with the provided bet.
    ///
    /// A same-side increase sets the price to the stake-weighted mean of the decimal odds.
    pub fn position_increase(&mut self, bet: &Bet) {
        if self.side().is_none() {
            self.price = bet.price;
        } else {
            let abs_self_exposure = self.exposure.abs();
            let abs_bet_exposure = bet.exposure().abs();
            // The mean is only meaningful for a same-side bet with well-formed odds:
            // `add_bet` routes the opposite side to `position_decrease`, but this
            // method is public, and `Bet` enforces neither a positive price nor a
            // positive stake. These conditions also guarantee a positive denominator
            // below. Anything else keeps the price it had.
            if self.side() == Some(bet.side)
                && self.price > Decimal::ZERO
                && bet.price > Decimal::ZERO
                && bet.stake > Decimal::ZERO
            {
                let total_stake = abs_self_exposure / self.price + bet.stake;
                self.price = (abs_self_exposure + abs_bet_exposure) / total_stake;
            }
        }
        self.exposure += bet.exposure();
    }

    /// Decreases the position with the provided bet, updating exposure and realized P&L.
    ///
    /// # Panics
    ///
    /// Panics if there is no current side (empty position) when unwrapping the side.
    pub fn position_decrease(&mut self, bet: &Bet) {
        let abs_bet_exposure = bet.exposure().abs();
        let abs_self_exposure = self.exposure.abs();

        match abs_bet_exposure.cmp(&abs_self_exposure) {
            std::cmp::Ordering::Less => {
                let decreasing_volume = abs_bet_exposure / self.price;
                let current_side = self.side().unwrap();
                let decreasing_bet = Bet::new(self.price, decreasing_volume, current_side);
                let pnl = calc_bets_pnl(&[bet.clone(), decreasing_bet]);
                self.realized_pnl += pnl;
                self.exposure += bet.exposure();
            }
            std::cmp::Ordering::Greater => {
                if let Some(self_bet) = self.as_bet() {
                    let pnl = calc_bets_pnl(&[bet.clone(), self_bet]);
                    self.realized_pnl += pnl;
                }
                self.price = bet.price;
                self.exposure += bet.exposure();
            }
            std::cmp::Ordering::Equal => {
                if let Some(self_bet) = self.as_bet() {
                    let pnl = calc_bets_pnl(&[bet.clone(), self_bet]);
                    self.realized_pnl += pnl;
                }
                self.price = Decimal::ZERO;
                self.exposure = Decimal::ZERO;
            }
        }
    }

    /// Calculates the unrealized profit and loss given a current price.
    ///
    /// # Panics
    ///
    /// Panics if flattening or marking the position overflows or divides by zero.
    #[must_use]
    pub fn unrealized_pnl(&self, price: Decimal) -> Decimal {
        self.unrealized_pnl_checked(price)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Calculates the unrealized profit and loss given a current price.
    ///
    /// # Errors
    ///
    /// Returns an error if flattening or marking the position overflows or divides by zero.
    pub fn unrealized_pnl_checked(&self, price: Decimal) -> anyhow::Result<Decimal> {
        if self.side().is_none() {
            return Ok(Decimal::ZERO);
        }
        let Some(flattening_bet) = self.flattening_bet_checked(price)? else {
            return Ok(Decimal::ZERO);
        };
        let Some(self_bet) = self.as_bet_checked()? else {
            return Ok(Decimal::ZERO);
        };
        calc_bets_pnl_checked(&[flattening_bet, self_bet])
    }

    /// Returns the total profit and loss (realized plus unrealized) given a current price.
    ///
    /// # Panics
    ///
    /// Panics if [`Self::unrealized_pnl`] panics or the sum overflows.
    #[must_use]
    pub fn total_pnl(&self, price: Decimal) -> Decimal {
        self.total_pnl_checked(price)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the total profit and loss (realized plus unrealized) given a current price.
    ///
    /// # Errors
    ///
    /// Returns an error if unrealized PnL cannot be computed or the sum overflows.
    pub fn total_pnl_checked(&self, price: Decimal) -> anyhow::Result<Decimal> {
        checked_add(self.realized_pnl, self.unrealized_pnl_checked(price)?)
    }

    /// Creates a bet that would flatten (neutralize) the current position.
    ///
    /// # Panics
    ///
    /// Panics if the position has a side and `price` is zero, or if the
    /// calculation overflows.
    #[must_use]
    pub fn flattening_bet(&self, price: Decimal) -> Option<Bet> {
        self.flattening_bet_checked(price)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Creates a bet that would flatten (neutralize) the current position.
    ///
    /// # Errors
    ///
    /// Returns an error if the position has a side and `price` is zero, or if
    /// the calculation overflows.
    pub fn flattening_bet_checked(&self, price: Decimal) -> anyhow::Result<Option<Bet>> {
        let Some(side) = self.side() else {
            return Ok(None);
        };
        check_nonzero_denominator(price, "price")?;
        let stake = match side {
            BetSide::Back => checked_div(self.exposure, price)?,
            BetSide::Lay => checked_div(-self.exposure, price)?,
        };
        Ok(Some(Bet::new(price, stake, side.opposite())))
    }

    /// Resets the bet position to its initial state.
    pub fn reset(&mut self) {
        self.price = Decimal::ZERO;
        self.exposure = Decimal::ZERO;
        self.realized_pnl = Decimal::ZERO;
        self.bets.clear();
    }
}

impl Display for BetPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BetPosition(price: {:.2}, exposure: {:.2}, realized_pnl: {:.2})",
            self.price, self.exposure, self.realized_pnl
        )
    }
}

/// Calculates the combined profit and loss for a slice of bets.
///
/// # Panics
///
/// Panics if a payoff or the running total overflows.
#[must_use]
pub fn calc_bets_pnl(bets: &[Bet]) -> Decimal {
    calc_bets_pnl_checked(bets).unwrap_or_else(|e| panic!("{e}"))
}

/// Calculates the combined profit and loss for a slice of bets.
///
/// # Errors
///
/// Returns an error if a payoff or the running total overflows.
pub fn calc_bets_pnl_checked(bets: &[Bet]) -> anyhow::Result<Decimal> {
    bets.iter().try_fold(Decimal::ZERO, |acc, bet| {
        checked_add(acc, bet.outcome_win_payoff_checked()?)
    })
}

/// Checks that `probability` is non-zero.
///
/// # Errors
///
/// Returns an error if `probability` is zero.
pub fn check_probability_non_zero(probability: Decimal) -> anyhow::Result<()> {
    if probability.is_zero() {
        anyhow::bail!("invalid probability: must be non-zero")
    }
    Ok(())
}

/// Checks that `probability` is invertible (not equal to 1.0).
///
/// # Errors
///
/// Returns an error if `probability` is 1.0.
pub fn check_probability_invertible(probability: Decimal) -> anyhow::Result<()> {
    if probability == Decimal::ONE {
        anyhow::bail!("invalid probability: must not be 1.0 (inverse would be zero)")
    }
    Ok(())
}

/// Converts a probability and volume into a Bet.
///
/// For a BUY side, this creates a BACK bet; for SELL, a LAY bet.
///
/// # Errors
///
/// Returns an error if `probability` is zero or the conversion overflows.
pub fn probability_to_bet(
    probability: Decimal,
    volume: Decimal,
    side: OrderSideSpecified,
) -> anyhow::Result<Bet> {
    check_probability_non_zero(probability)?;
    let price = checked_div(Decimal::ONE, probability)?;
    let stake = checked_div(volume, price)?;
    let bet = match side {
        OrderSideSpecified::Buy => Bet::new(price, stake, BetSide::Back),
        OrderSideSpecified::Sell => Bet::new(price, stake, BetSide::Lay),
    };
    Ok(bet)
}

/// Converts a probability and volume into a Bet using the inverse probability.
///
/// The side is also inverted (BUY becomes SELL and vice versa).
///
/// # Errors
///
/// Returns an error if `probability` is 1.0 or its inverse is zero.
pub fn inverse_probability_to_bet(
    probability: Decimal,
    volume: Decimal,
    side: OrderSideSpecified,
) -> anyhow::Result<Bet> {
    check_probability_invertible(probability)?;
    let inverse_probability = checked_sub(Decimal::ONE, probability)?;
    let inverse_side = match side {
        OrderSideSpecified::Buy => OrderSideSpecified::Sell,
        OrderSideSpecified::Sell => OrderSideSpecified::Buy,
    };
    probability_to_bet(inverse_probability, volume, inverse_side)
}

fn check_odds_gt_one(price: Decimal) -> anyhow::Result<()> {
    if price <= Decimal::ONE {
        anyhow::bail!("Price must be greater than 1.0 for lay liability calculation, was {price}");
    }
    Ok(())
}

fn check_nonzero_denominator(value: Decimal, name: &str) -> anyhow::Result<()> {
    if value.is_zero() {
        anyhow::bail!("invalid {name}: must be non-zero")
    }
    Ok(())
}

/// Converts [`OrderSide`] into a specified side for betting conversions.
///
/// # Errors
///
/// Returns an error if `side` is [`OrderSide::NoOrderSide`].
pub fn specified_order_side(side: OrderSide) -> anyhow::Result<OrderSideSpecified> {
    match side {
        OrderSide::Buy => Ok(OrderSideSpecified::Buy),
        OrderSide::Sell => Ok(OrderSideSpecified::Sell),
        OrderSide::NoOrderSide => {
            anyhow::bail!("invalid OrderSide: must be Buy or Sell, was {side}")
        }
    }
}

fn checked_add(lhs: Decimal, rhs: Decimal) -> anyhow::Result<Decimal> {
    lhs.checked_add(rhs)
        .ok_or_else(|| anyhow::anyhow!("Decimal overflow adding {lhs} and {rhs}"))
}

fn checked_sub(lhs: Decimal, rhs: Decimal) -> anyhow::Result<Decimal> {
    lhs.checked_sub(rhs)
        .ok_or_else(|| anyhow::anyhow!("Decimal overflow subtracting {rhs} from {lhs}"))
}

fn checked_mul(lhs: Decimal, rhs: Decimal) -> anyhow::Result<Decimal> {
    lhs.checked_mul(rhs)
        .ok_or_else(|| anyhow::anyhow!("Decimal overflow multiplying {lhs} by {rhs}"))
}

fn checked_div(lhs: Decimal, rhs: Decimal) -> anyhow::Result<Decimal> {
    check_nonzero_denominator(rhs, "divisor")?;
    lhs.checked_div(rhs)
        .ok_or_else(|| anyhow::anyhow!("Decimal overflow dividing {lhs} by {rhs}"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;

    fn dec_str(s: &str) -> Decimal {
        s.parse::<Decimal>().expect("Failed to parse Decimal")
    }

    #[rstest]
    #[should_panic(expected = "Liability-based betting is only applicable for Lay side.")]
    fn test_from_liability_panics_on_back_side() {
        let _ = Bet::from_liability(dec!(2.0), dec!(100.0), BetSide::Back);
    }

    #[rstest]
    fn test_bet_creation() {
        let price = dec!(2.0);
        let stake = dec!(100.0);
        let side = BetSide::Back;
        let bet = Bet::new(price, stake, side);
        assert_eq!(bet.price, price);
        assert_eq!(bet.stake, stake);
        assert_eq!(bet.side, side);
    }

    #[rstest]
    fn test_display_bet() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let formatted = format!("{bet}");
        assert!(formatted.contains("Back"));
        assert!(formatted.contains("2.00"));
        assert!(formatted.contains("100.00"));
    }

    #[rstest]
    fn test_bet_exposure_back() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let exposure = bet.exposure();
        assert_eq!(exposure, dec!(200.0));
    }

    #[rstest]
    fn test_bet_exposure_lay() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let exposure = bet.exposure();
        assert_eq!(exposure, dec!(-200.0));
    }

    #[rstest]
    fn test_bet_liability_back() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let liability = bet.liability();
        assert_eq!(liability, dec!(100.0));
    }

    #[rstest]
    fn test_bet_liability_lay() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let liability = bet.liability();
        assert_eq!(liability, dec!(100.0));
    }

    #[rstest]
    fn test_bet_profit_back() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let profit = bet.profit();
        assert_eq!(profit, dec!(100.0));
    }

    #[rstest]
    fn test_bet_profit_lay() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let profit = bet.profit();
        assert_eq!(profit, dec!(100.0));
    }

    #[rstest]
    fn test_outcome_win_payoff_back() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let win_payoff = bet.outcome_win_payoff();
        assert_eq!(win_payoff, dec!(100.0));
    }

    #[rstest]
    fn test_outcome_win_payoff_lay() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let win_payoff = bet.outcome_win_payoff();
        assert_eq!(win_payoff, dec!(-100.0));
    }

    #[rstest]
    fn test_outcome_lose_payoff_back() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let lose_payoff = bet.outcome_lose_payoff();
        assert_eq!(lose_payoff, dec!(-100.0));
    }

    #[rstest]
    fn test_outcome_lose_payoff_lay() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let lose_payoff = bet.outcome_lose_payoff();
        assert_eq!(lose_payoff, dec!(100.0));
    }

    #[rstest]
    fn test_hedging_stake_back() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let hedging_stake = bet.hedging_stake(dec!(1.5));
        // Expected: (2.0/1.5)*100 = 133.3333333333...
        assert_eq!(hedging_stake.round_dp(8), dec_str("133.33333333"));
    }

    #[rstest]
    fn test_hedging_bet_lay() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let hedge_bet = bet.hedging_bet(dec!(1.5));
        assert_eq!(hedge_bet.side, BetSide::Back);
        assert_eq!(hedge_bet.price, dec!(1.5));
        assert_eq!(hedge_bet.stake.round_dp(8), dec_str("133.33333333"));
    }

    #[rstest]
    fn test_bet_position_initialization() {
        let position = BetPosition::default();
        assert_eq!(position.price, dec!(0.0));
        assert_eq!(position.exposure, dec!(0.0));
        assert_eq!(position.realized_pnl, dec!(0.0));
    }

    #[rstest]
    fn test_display_bet_position() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        position.add_bet(bet);
        let formatted = format!("{position}");

        assert!(formatted.contains("price"));
        assert!(formatted.contains("exposure"));
        assert!(formatted.contains("realized_pnl"));
    }

    #[rstest]
    fn test_as_bet() {
        let mut position = BetPosition::default();
        // Add a BACK bet so the position has exposure
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        position.add_bet(bet);
        let as_bet = position.as_bet().expect("Expected a bet representation");

        assert_eq!(as_bet.price, position.price);
        assert_eq!(as_bet.stake, position.exposure / position.price);
        assert_eq!(as_bet.side, BetSide::Back);
    }

    #[rstest]
    fn test_reset_position() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        position.add_bet(bet);
        assert_ne!(position.exposure, dec!(0.0));
        assert!(!position.bets().is_empty());
        position.reset();

        // After reset, the position should be cleared
        assert_eq!(position.price, dec!(0.0));
        assert_eq!(position.exposure, dec!(0.0));
        assert_eq!(position.realized_pnl, dec!(0.0));
        assert!(position.bets().is_empty());
    }

    #[rstest]
    fn test_bet_position_side_none() {
        let position = BetPosition::default();
        assert!(position.side().is_none());
    }

    #[rstest]
    fn test_bet_position_side_back() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        position.add_bet(bet);
        assert_eq!(position.side(), Some(BetSide::Back));
    }

    #[rstest]
    fn test_bet_position_side_lay() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        position.add_bet(bet);
        assert_eq!(position.side(), Some(BetSide::Lay));
    }

    #[rstest]
    fn test_position_increase_back() {
        let mut position = BetPosition::default();
        let bet1 = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let bet2 = Bet::new(dec!(2.0), dec!(50.0), BetSide::Back);
        position.add_bet(bet1);
        position.add_bet(bet2);
        // Expected exposure = 200 + 100 = 300
        assert_eq!(position.exposure, dec!(300.0));
    }

    #[rstest]
    fn test_position_increase_cancelling_stakes_preserves_price() {
        let mut position = BetPosition::default();
        position.add_bet(Bet::new(dec!(2.0), dec!(100.0), BetSide::Back));
        // `Bet` does not enforce a positive stake, and a cancelling one drives the
        // aggregate stake to zero.
        position.add_bet(Bet::new(dec!(2.0), dec!(-100.0), BetSide::Back));

        assert_eq!(position.price, dec!(2.0));
        assert_eq!(position.exposure, dec!(0.0));
    }

    #[rstest]
    fn test_position_increase_negative_stake_preserves_price() {
        let mut position = BetPosition::default();
        position.add_bet(Bet::new(dec!(2.0), dec!(100.0), BetSide::Back));
        // Leaves a positive aggregate stake, so only the stake sign rejects it.
        position.add_bet(Bet::new(dec!(3.0), dec!(-50.0), BetSide::Back));

        assert_eq!(position.price, dec!(2.0));
        assert_eq!(position.exposure, dec!(50.0));
    }

    #[rstest]
    fn test_position_increase_opposite_side_preserves_price() {
        let mut position = BetPosition::default();
        position.add_bet(Bet::new(dec!(2.0), dec!(100.0), BetSide::Back));
        // `add_bet` would route this to `position_decrease`; the public method is
        // callable directly and must not average across sides.
        position.position_increase(&Bet::new(dec!(3.0), dec!(50.0), BetSide::Lay));

        assert_eq!(position.price, dec!(2.0));
    }

    #[rstest]
    #[case(dec!(0.0))]
    #[case(dec!(-3.0))]
    fn test_position_increase_non_positive_incoming_price_preserves_price(#[case] price: Decimal) {
        let mut position = BetPosition::default();
        position.add_bet(Bet::new(dec!(2.0), dec!(100.0), BetSide::Back));
        // Both stakes are positive, so only the incoming price rejects it.
        position.add_bet(Bet::new(price, dec!(50.0), BetSide::Back));

        assert_eq!(position.price, dec!(2.0));
    }

    #[rstest]
    fn test_position_increase_non_positive_current_price_preserves_price() {
        let mut position = BetPosition::default();
        // `Bet` enforces no positive price, so an opening bet can leave a nonempty
        // position whose current price is negative.
        position.add_bet(Bet::new(dec!(-3.0), dec!(100.0), BetSide::Lay));
        assert_eq!(position.side(), Some(BetSide::Back));

        // Same side, positive incoming price and stake, so only the current price
        // rejects it: the aggregate stake would be 300 / -3 + 100, and averaging
        // would divide by zero.
        position.add_bet(Bet::new(dec!(2.0), dec!(100.0), BetSide::Back));

        assert_eq!(position.price, dec!(-3.0));
        assert_eq!(position.exposure, dec!(500.0));
    }

    #[rstest]
    fn test_position_increase_back_averages_price_and_conserves_pnl() {
        let mut position = BetPosition::default();
        let bet1 = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let bet2 = Bet::new(dec!(4.0), dec!(50.0), BetSide::Back);
        let settlement_price = dec!(3.0);
        let constituent_pnl = calc_bets_pnl(&[
            bet1.clone(),
            bet1.hedging_bet(settlement_price),
            bet2.clone(),
            bet2.hedging_bet(settlement_price),
        ]);

        position.add_bet(bet1);
        position.add_bet(bet2);

        assert_eq!(position.price, dec!(400.0) / dec!(150.0));
        assert_eq!(
            position.total_pnl(settlement_price).round_dp(8),
            constituent_pnl.round_dp(8)
        );
    }

    #[rstest]
    fn test_position_increase_lay() {
        let mut position = BetPosition::default();
        let bet1 = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let bet2 = Bet::new(dec!(2.0), dec!(50.0), BetSide::Lay);
        position.add_bet(bet1);
        position.add_bet(bet2);
        // exposure = -200 + (-100) = -300
        assert_eq!(position.exposure, dec!(-300.0));
    }

    #[rstest]
    fn test_position_increase_lay_averages_price_and_conserves_pnl() {
        let mut position = BetPosition::default();
        let bet1 = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let bet2 = Bet::new(dec!(4.0), dec!(50.0), BetSide::Lay);
        let settlement_price = dec!(3.0);
        let constituent_pnl = calc_bets_pnl(&[
            bet1.clone(),
            bet1.hedging_bet(settlement_price),
            bet2.clone(),
            bet2.hedging_bet(settlement_price),
        ]);

        position.add_bet(bet1);
        position.add_bet(bet2);

        assert_eq!(position.price, dec!(400.0) / dec!(150.0));
        assert_eq!(
            position.total_pnl(settlement_price).round_dp(8),
            constituent_pnl.round_dp(8)
        );
    }

    #[rstest]
    fn test_position_back_then_lay() {
        let mut position = BetPosition::default();
        let bet1 = Bet::new(dec!(3.0), dec!(100_000), BetSide::Back);
        let bet2 = Bet::new(dec!(2.0), dec!(10_000), BetSide::Lay);
        position.add_bet(bet1);
        position.add_bet(bet2);

        assert_eq!(position.exposure, dec!(280_000.0));
        assert_eq!(position.realized_pnl(), dec!(3333.333333333333333333333333));
        assert_eq!(
            position.unrealized_pnl(dec!(4.0)),
            dec!(-23333.33333333333333333333334)
        );
    }

    #[rstest]
    fn test_position_lay_then_back() {
        let mut position = BetPosition::default();
        let bet1 = Bet::new(dec!(2.0), dec!(10_000), BetSide::Lay);
        let bet2 = Bet::new(dec!(3.0), dec!(100_000), BetSide::Back);
        position.add_bet(bet1);
        position.add_bet(bet2);

        assert_eq!(position.exposure, dec!(280_000.0));
        assert_eq!(position.realized_pnl(), dec!(190_000));
        assert_eq!(
            position.unrealized_pnl(dec!(4.0)),
            dec!(-23333.33333333333333333333334)
        );
    }

    #[rstest]
    fn test_position_flip() {
        let mut position = BetPosition::default();
        let back_bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back); // exposure +200
        let lay_bet = Bet::new(dec!(2.0), dec!(150.0), BetSide::Lay); // exposure -300
        position.add_bet(back_bet);
        position.add_bet(lay_bet);
        // Net exposure: 200 + (-300) = -100 → side becomes Lay.
        assert_eq!(position.side(), Some(BetSide::Lay));
        assert_eq!(position.exposure, dec!(-100.0));
    }

    #[rstest]
    fn test_position_flat() {
        let mut position = BetPosition::default();
        let back_bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back); // exposure +200
        let lay_bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay); // exposure -200
        position.add_bet(back_bet);
        position.add_bet(lay_bet);
        assert!(position.side().is_none());
        assert_eq!(position.exposure, dec!(0.0));
    }

    #[rstest]
    fn test_unrealized_pnl_negative() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back); // exposure 200
        position.add_bet(bet);
        // As computed: flattening bet (Lay at 2.5) gives stake = 80 and win payoff = -120, plus original bet win payoff = 100 → -20
        let unrealized_pnl = position.unrealized_pnl(dec!(2.5));
        assert_eq!(unrealized_pnl, dec!(-20.0));
    }

    #[rstest]
    fn test_total_pnl() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        position.add_bet(bet);
        position.realized_pnl = dec!(10.0);
        let total_pnl = position.total_pnl(dec!(2.5));
        // Expected realized (10) + unrealized (-20) = -10
        assert_eq!(total_pnl, dec!(-10.0));
    }

    #[rstest]
    fn test_flattening_bet_back_profit() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        position.add_bet(bet);
        let flattening_bet = position
            .flattening_bet(dec!(1.6))
            .expect("expected a flattening bet");
        assert_eq!(flattening_bet.side, BetSide::Lay);
        assert_eq!(flattening_bet.stake, dec_str("125"));
    }

    #[rstest]
    fn test_flattening_bet_back_hack() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        position.add_bet(bet);
        let flattening_bet = position
            .flattening_bet(dec!(2.5))
            .expect("expected a flattening bet");
        assert_eq!(flattening_bet.side, BetSide::Lay);
        // Expected stake ~80
        assert_eq!(flattening_bet.stake, dec!(80.0));
    }

    #[rstest]
    fn test_flattening_bet_lay() {
        let mut position = BetPosition::default();
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        position.add_bet(bet);
        let flattening_bet = position
            .flattening_bet(dec!(1.5))
            .expect("expected a flattening bet");
        assert_eq!(flattening_bet.side, BetSide::Back);
        assert_eq!(flattening_bet.stake.round_dp(8), dec_str("133.33333333"));
    }

    #[rstest]
    fn test_realized_pnl_flattening() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back); // profit = 400
        let lay = Bet::new(dec!(4.0), dec!(125.0), BetSide::Lay); // outcome win payoff = -375
        let mut position = BetPosition::default();
        position.add_bet(back);
        position.add_bet(lay);
        // Expected realized pnl = 25
        assert_eq!(position.realized_pnl, dec!(25.0));
    }

    #[rstest]
    fn test_realized_pnl_single_side() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back);
        let mut position = BetPosition::default();
        position.add_bet(back);
        // No opposing bet → pnl remains 0
        assert_eq!(position.realized_pnl, dec!(0.0));
    }

    #[rstest]
    fn test_realized_pnl_open_position() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back); // exposure +500
        let lay = Bet::new(dec!(4.0), dec!(100.0), BetSide::Lay); // exposure -400
        let mut position = BetPosition::default();
        position.add_bet(back);
        position.add_bet(lay);
        // Expected realized pnl = 20
        assert_eq!(position.realized_pnl, dec!(20.0));
    }

    #[rstest]
    fn test_realized_pnl_partial_close() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back); // exposure +500
        let lay = Bet::new(dec!(4.0), dec!(110.0), BetSide::Lay); // exposure -440
        let mut position = BetPosition::default();
        position.add_bet(back);
        position.add_bet(lay);
        // Expected realized pnl = 22
        assert_eq!(position.realized_pnl, dec!(22.0));
    }

    #[rstest]
    fn test_realized_pnl_flipping() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back); // exposure +500
        let lay = Bet::new(dec!(4.0), dec!(130.0), BetSide::Lay); // exposure -520
        let mut position = BetPosition::default();
        position.add_bet(back);
        position.add_bet(lay);
        // Expected realized pnl = 10
        assert_eq!(position.realized_pnl, dec!(10.0));
    }

    #[rstest]
    fn test_unrealized_pnl_positive() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back); // exposure +500
        let mut position = BetPosition::default();
        position.add_bet(back);
        let unrealized_pnl = position.unrealized_pnl(dec!(4.0));
        // Expected unrealized pnl = 25
        assert_eq!(unrealized_pnl, dec!(25.0));
    }

    #[rstest]
    fn test_total_pnl_with_pnl() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back); // exposure +500
        let lay = Bet::new(dec!(4.0), dec!(120.0), BetSide::Lay); // exposure -480
        let mut position = BetPosition::default();
        position.add_bet(back);
        position.add_bet(lay);
        // After processing, realized pnl should be 24 and unrealized pnl 1.0
        let realized_pnl = position.realized_pnl;
        let unrealized_pnl = position.unrealized_pnl(dec!(4.0));
        let total_pnl = position.total_pnl(dec!(4.0));
        assert_eq!(realized_pnl, dec!(24.0));
        assert_eq!(unrealized_pnl, dec!(1.0));
        assert_eq!(total_pnl, dec!(25.0));
    }

    #[rstest]
    fn test_open_position_realized_unrealized() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back); // exposure +500
        let lay = Bet::new(dec!(4.0), dec!(100.0), BetSide::Lay); // exposure -400
        let mut position = BetPosition::default();
        position.add_bet(back);
        position.add_bet(lay);
        let unrealized_pnl = position.unrealized_pnl(dec!(4.0));
        // Expected unrealized pnl = 5
        assert_eq!(unrealized_pnl, dec!(5.0));
    }

    #[rstest]
    fn test_unrealized_no_position() {
        let back = Bet::new(dec!(5.0), dec!(100.0), BetSide::Lay);
        let mut position = BetPosition::default();
        position.add_bet(back);
        let unrealized_pnl = position.unrealized_pnl(dec!(5.0));
        assert_eq!(unrealized_pnl, dec!(0.0));
    }

    #[rstest]
    fn test_calc_bets_pnl_single_back_bet() {
        let bet = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back);
        let pnl = calc_bets_pnl(&[bet]);
        assert_eq!(pnl, dec!(400.0));
    }

    #[rstest]
    fn test_calc_bets_pnl_single_lay_bet() {
        let bet = Bet::new(dec!(4.0), dec!(100.0), BetSide::Lay);
        let pnl = calc_bets_pnl(&[bet]);
        assert_eq!(pnl, dec!(-300.0));
    }

    #[rstest]
    fn test_calc_bets_pnl_multiple_bets() {
        let back_bet = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back);
        let lay_bet = Bet::new(dec!(4.0), dec!(100.0), BetSide::Lay);
        let pnl = calc_bets_pnl(&[back_bet, lay_bet]);
        let expected = dec!(400.0) + dec!(-300.0);
        assert_eq!(pnl, expected);
    }

    #[rstest]
    fn test_calc_bets_pnl_mixed_bets() {
        let back_bet1 = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back);
        let back_bet2 = Bet::new(dec!(2.0), dec!(50.0), BetSide::Back);
        let lay_bet1 = Bet::new(dec!(3.0), dec!(75.0), BetSide::Lay);
        let pnl = calc_bets_pnl(&[back_bet1, back_bet2, lay_bet1]);
        let expected = dec!(400.0) + dec!(50.0) + dec!(-150.0);
        assert_eq!(pnl, expected);
    }

    #[rstest]
    fn test_calc_bets_pnl_no_bets() {
        let bets: Vec<Bet> = vec![];
        let pnl = calc_bets_pnl(&bets);
        assert_eq!(pnl, dec!(0.0));
    }

    #[rstest]
    fn test_calc_bets_pnl_zero_outcome() {
        let back_bet = Bet::new(dec!(5.0), dec!(100.0), BetSide::Back);
        let lay_bet = Bet::new(dec!(5.0), dec!(100.0), BetSide::Lay);
        let pnl = calc_bets_pnl(&[back_bet, lay_bet]);
        assert_eq!(pnl, dec!(0.0));
    }

    #[rstest]
    fn test_probability_to_bet_back_simple() {
        // Using OrderSideSpecified in place of ProbSide.
        let bet = probability_to_bet(dec!(0.50), dec!(50.0), OrderSideSpecified::Buy).unwrap();
        let expected = Bet::new(dec!(2.0), dec!(25.0), BetSide::Back);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec!(25.0));
        assert_eq!(bet.outcome_lose_payoff(), dec!(-25.0));
    }

    #[rstest]
    fn test_probability_to_bet_back_high_prob() {
        let bet = probability_to_bet(dec!(0.64), dec!(50.0), OrderSideSpecified::Buy).unwrap();
        let expected = Bet::new(dec!(1.5625), dec!(32.0), BetSide::Back);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec!(18.0));
        assert_eq!(bet.outcome_lose_payoff(), dec!(-32.0));
    }

    #[rstest]
    fn test_probability_to_bet_back_low_prob() {
        let bet = probability_to_bet(dec!(0.40), dec!(50.0), OrderSideSpecified::Buy).unwrap();
        let expected = Bet::new(dec!(2.5), dec!(20.0), BetSide::Back);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec!(30.0));
        assert_eq!(bet.outcome_lose_payoff(), dec!(-20.0));
    }

    #[rstest]
    fn test_probability_to_bet_sell() {
        let bet = probability_to_bet(dec!(0.80), dec!(50.0), OrderSideSpecified::Sell).unwrap();
        let expected = Bet::new(dec_str("1.25"), dec_str("40"), BetSide::Lay);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec_str("-10"));
        assert_eq!(bet.outcome_lose_payoff(), dec_str("40"));
    }

    #[rstest]
    fn test_inverse_probability_to_bet() {
        // Original bet with SELL side
        let original_bet =
            probability_to_bet(dec!(0.80), dec!(100.0), OrderSideSpecified::Sell).unwrap();
        // Equivalent reverse bet by buying the inverse probability
        let reverse_bet =
            probability_to_bet(dec!(0.20), dec!(100.0), OrderSideSpecified::Buy).unwrap();
        let inverse_bet =
            inverse_probability_to_bet(dec!(0.80), dec!(100.0), OrderSideSpecified::Sell).unwrap();

        assert_eq!(
            original_bet.outcome_win_payoff(),
            reverse_bet.outcome_lose_payoff(),
        );
        assert_eq!(
            original_bet.outcome_win_payoff(),
            inverse_bet.outcome_lose_payoff(),
        );
        assert_eq!(
            original_bet.outcome_lose_payoff(),
            reverse_bet.outcome_win_payoff(),
        );
        assert_eq!(
            original_bet.outcome_lose_payoff(),
            inverse_bet.outcome_win_payoff(),
        );
    }

    #[rstest]
    fn test_inverse_probability_to_bet_example2() {
        let original_bet =
            probability_to_bet(dec!(0.64), dec!(50.0), OrderSideSpecified::Sell).unwrap();
        let inverse_bet =
            inverse_probability_to_bet(dec!(0.64), dec!(50.0), OrderSideSpecified::Sell).unwrap();

        assert_eq!(original_bet.stake, dec!(32.0));
        assert_eq!(original_bet.outcome_win_payoff(), dec!(-18.0));
        assert_eq!(original_bet.outcome_lose_payoff(), dec!(32.0));

        assert_eq!(inverse_bet.stake, dec!(18.0));
        assert_eq!(inverse_bet.outcome_win_payoff(), dec!(32.0));
        assert_eq!(inverse_bet.outcome_lose_payoff(), dec!(-18.0));
    }

    #[rstest]
    fn test_from_liability_checked_rejects_back_side() {
        let err = Bet::from_liability_checked(dec!(2.0), dec!(100.0), BetSide::Back).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Liability-based betting is only applicable for Lay side."
        );
    }

    #[rstest]
    #[case(dec!(1.0))]
    #[case(dec!(0.0))]
    #[case(dec!(-1.0))]
    fn test_from_liability_checked_rejects_odds_at_or_below_one(#[case] price: Decimal) {
        let err = Bet::from_liability_checked(price, dec!(100.0), BetSide::Lay).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("Price must be greater than 1.0 for lay liability calculation, was {price}")
        );
    }

    #[rstest]
    fn test_from_stake_or_liability_checked_rejects_lay_odds_at_one() {
        let err =
            Bet::from_stake_or_liability_checked(dec!(1.0), dec!(100.0), BetSide::Lay).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Price must be greater than 1.0 for lay liability calculation, was 1.0"
        );
    }

    #[rstest]
    fn test_from_stake_or_liability_checked_allows_back_odds_at_one() {
        let bet =
            Bet::from_stake_or_liability_checked(dec!(1.0), dec!(10.0), BetSide::Back).unwrap();
        assert_eq!(bet.price(), dec!(1.0));
        assert_eq!(bet.stake(), dec!(10.0));
        assert_eq!(bet.side(), BetSide::Back);
        assert_eq!(bet.exposure_checked().unwrap(), dec!(10.0));
        assert_eq!(bet.profit_checked().unwrap(), dec!(0.0));
    }

    #[rstest]
    fn test_from_liability_checked_preserves_stake_identity() {
        let bet = Bet::from_liability_checked(dec!(2.5), dec!(15.0), BetSide::Lay).unwrap();
        assert_eq!(bet.stake(), dec!(10.0));
        assert_eq!(bet.liability_checked().unwrap(), dec!(15.0));
    }

    #[rstest]
    fn test_hedging_stake_checked_rejects_zero_price() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Back);
        let err = bet.hedging_stake_checked(Decimal::ZERO).unwrap_err();
        assert_eq!(err.to_string(), "invalid divisor: must be non-zero");
    }

    #[rstest]
    fn test_hedging_bet_checked_rejects_zero_price() {
        let bet = Bet::new(dec!(2.0), dec!(100.0), BetSide::Lay);
        let err = bet.hedging_bet_checked(Decimal::ZERO).unwrap_err();
        assert_eq!(err.to_string(), "invalid divisor: must be non-zero");
    }

    #[rstest]
    fn test_exposure_checked_rejects_overflow() {
        let bet = Bet::new(Decimal::MAX, dec!(2.0), BetSide::Back);
        let err = bet.exposure_checked().unwrap_err();
        assert!(err.to_string().starts_with("Decimal overflow multiplying"));
    }

    #[rstest]
    fn test_liability_checked_rejects_overflow() {
        let bet = Bet::new(Decimal::MAX, dec!(2.0), BetSide::Lay);
        let err = bet.liability_checked().unwrap_err();
        assert!(err.to_string().starts_with("Decimal overflow multiplying"));
    }

    #[rstest]
    fn test_flattening_bet_checked_rejects_zero_price() {
        let mut position = BetPosition::default();
        position.add_bet(Bet::new(dec!(2.0), dec!(100.0), BetSide::Back));
        let err = position.flattening_bet_checked(Decimal::ZERO).unwrap_err();
        assert_eq!(err.to_string(), "invalid price: must be non-zero");
    }

    #[rstest]
    fn test_add_bet_checked_matches_infallible_decrease() {
        let back = Bet::new(dec!(3.0), dec!(100_000), BetSide::Back);
        let lay = Bet::new(dec!(2.0), dec!(10_000), BetSide::Lay);
        let mut expected = BetPosition::default();
        expected.add_bet(back.clone());
        expected.add_bet(lay.clone());

        let mut position = BetPosition::default();
        position.add_bet_checked(back).unwrap();
        position.add_bet_checked(lay).unwrap();

        assert_eq!(position.price(), expected.price());
        assert_eq!(position.exposure(), expected.exposure());
        assert_eq!(position.realized_pnl(), expected.realized_pnl());
        assert_eq!(position.bets(), expected.bets());
    }

    #[rstest]
    fn test_add_bet_checked_rejects_overflow_and_leaves_position_unchanged() {
        let mut position = BetPosition::default();
        position
            .add_bet_checked(Bet::new(dec!(2.0), dec!(100.0), BetSide::Back))
            .unwrap();
        let before_price = position.price();
        let before_exposure = position.exposure();
        let before_len = position.bets().len();

        let err = position
            .add_bet_checked(Bet::new(Decimal::MAX, dec!(2.0), BetSide::Back))
            .unwrap_err();

        assert!(err.to_string().starts_with("Decimal overflow multiplying"));
        assert_eq!(position.price(), before_price);
        assert_eq!(position.exposure(), before_exposure);
        assert_eq!(position.bets().len(), before_len);
    }

    #[rstest]
    fn test_specified_order_side_rejects_unspecified() {
        let err = specified_order_side(OrderSide::NoOrderSide).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid OrderSide: must be Buy or Sell, was NO_ORDER_SIDE"
        );
    }

    #[rstest]
    fn test_checked_methods_preserve_valid_identities() {
        let back = Bet::new(dec!(2.5), dec!(10.0), BetSide::Back);
        let hedge = back.hedging_bet_checked(dec!(1.5)).unwrap();

        assert_eq!(back.exposure_checked().unwrap(), back.exposure());
        assert_eq!(back.liability_checked().unwrap(), back.liability());
        assert_eq!(back.profit_checked().unwrap(), back.profit());
        assert_eq!(
            back.outcome_win_payoff_checked().unwrap(),
            back.outcome_win_payoff()
        );
        assert_eq!(
            back.outcome_lose_payoff_checked().unwrap(),
            back.outcome_lose_payoff()
        );
        assert_eq!(hedge, back.hedging_bet(dec!(1.5)));
        assert_eq!(
            calc_bets_pnl_checked(&[back.clone(), hedge.clone()]).unwrap(),
            calc_bets_pnl(&[back, hedge])
        );
    }
}
