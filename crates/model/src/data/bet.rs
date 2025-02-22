// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::Display;

use rust_decimal::Decimal;

use crate::enums::{BetSide, OrderSideSpecified};

/// A bet in a betting market.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Bet {
    price: Decimal,
    stake: Decimal,
    side: BetSide,
}

impl Bet {
    /// Creates a new [`Bet`] instance.
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
    /// For `BetSide::Back` this calls [Self::from_stake] and for
    /// `BetSide::Lay` it calls [Self::from_liability].
    pub fn from_stake_or_liability(price: Decimal, volume: Decimal, side: BetSide) -> Self {
        match side {
            BetSide::Back => Self::from_stake(price, volume, side),
            BetSide::Lay => Self::from_liability(price, volume, side),
        }
    }

    /// Creates a bet from a given stake.
    pub fn from_stake(price: Decimal, stake: Decimal, side: BetSide) -> Self {
        Self::new(price, stake, side)
    }

    /// Creates a bet from a given liability.
    ///
    /// # Panics
    ///
    /// Panics if the side is not [BetSide::Lay].
    pub fn from_liability(price: Decimal, liability: Decimal, side: BetSide) -> Self {
        if side != BetSide::Lay {
            panic!("Liability-based betting is only applicable for Lay side.");
        }
        let adjusted_volume = liability / (price - Decimal::ONE);
        Self::new(price, adjusted_volume, side)
    }

    /// Returns the bet's exposure.
    ///
    /// For BACK bets, exposure is positive; for LAY bets, it is negative.
    pub fn exposure(&self) -> Decimal {
        match self.side {
            BetSide::Back => self.price * self.stake,
            BetSide::Lay => -self.price * self.stake,
        }
    }

    /// Returns the bet's liability.
    ///
    /// For BACK bets, liability equals the stake; for LAY bets, it is
    /// stake multiplied by (price - 1).
    pub fn liability(&self) -> Decimal {
        match self.side {
            BetSide::Back => self.stake,
            BetSide::Lay => self.stake * (self.price - Decimal::ONE),
        }
    }

    /// Returns the bet's profit.
    ///
    /// For BACK bets, profit is stake * (price - 1); for LAY bets it equals the stake.
    pub fn profit(&self) -> Decimal {
        match self.side {
            BetSide::Back => self.stake * (self.price - Decimal::ONE),
            BetSide::Lay => self.stake,
        }
    }

    /// Returns the outcome win payoff.
    ///
    /// For BACK bets this is the profit; for LAY bets it is the negative liability.
    pub fn outcome_win_payoff(&self) -> Decimal {
        match self.side {
            BetSide::Back => self.profit(),
            BetSide::Lay => -self.liability(),
        }
    }

    /// Returns the outcome lose payoff.
    ///
    /// For BACK bets this is the negative liability; for LAY bets it is the profit.
    pub fn outcome_lose_payoff(&self) -> Decimal {
        match self.side {
            BetSide::Back => -self.liability(),
            BetSide::Lay => self.profit(),
        }
    }

    /// Returns the hedging stake given a new price.
    pub fn hedging_stake(&self, price: Decimal) -> Decimal {
        match self.side {
            BetSide::Back => (self.price / price) * self.stake,
            BetSide::Lay => self.stake / (price / self.price),
        }
    }

    /// Creates a hedging bet for a given price.
    pub fn hedging_bet(&self, price: Decimal) -> Self {
        Self::new(price, self.hedging_stake(price), self.side.opposite())
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
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
    pub fn side(&self) -> Option<BetSide> {
        match self.exposure.cmp(&Decimal::ZERO) {
            std::cmp::Ordering::Less => Some(BetSide::Lay),
            std::cmp::Ordering::Greater => Some(BetSide::Back),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// Converts the current position into a single bet, if possible.
    pub fn as_bet(&self) -> Option<Bet> {
        self.side().map(|side| {
            let stake = match side {
                BetSide::Back => self.exposure / self.price,
                BetSide::Lay => -self.exposure / self.price,
            };
            Bet::new(self.price, stake, side)
        })
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

    /// Increases the position with the provided bet.
    pub fn position_increase(&mut self, bet: &Bet) {
        if self.side().is_none() {
            self.price = bet.price;
        }
        self.exposure += bet.exposure();
    }

    /// Decreases the position with the provided bet.
    ///
    /// This method calculates the realized PnL by comparing the incoming bet with
    /// a corresponding bet derived from the current position.
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
    pub fn unrealized_pnl(&self, price: Decimal) -> Decimal {
        if self.side().is_none() {
            Decimal::ZERO
        } else if let Some(flattening_bet) = self.flattening_bet(price) {
            if let Some(self_bet) = self.as_bet() {
                calc_bets_pnl(&[flattening_bet, self_bet])
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ZERO
        }
    }

    /// Returns the total profit and loss (realized plus unrealized) given a current price.
    pub fn total_pnl(&self, price: Decimal) -> Decimal {
        self.realized_pnl + self.unrealized_pnl(price)
    }

    /// Creates a bet that would flatten (neutralize) the current position.
    pub fn flattening_bet(&self, price: Decimal) -> Option<Bet> {
        self.side().map(|side| {
            let stake = match side {
                BetSide::Back => self.exposure / price,
                BetSide::Lay => -self.exposure / price,
            };
            // Use the opposite side to flatten the position.
            Bet::new(price, stake, side.opposite())
        })
    }

    /// Resets the bet position to its initial state.
    pub fn reset(&mut self) {
        self.price = Decimal::ZERO;
        self.exposure = Decimal::ZERO;
        self.realized_pnl = Decimal::ZERO;
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
pub fn calc_bets_pnl(bets: &[Bet]) -> Decimal {
    bets.iter()
        .fold(Decimal::ZERO, |acc, bet| acc + bet.outcome_win_payoff())
}

/// Converts a probability and volume into a Bet.
///
/// For a BUY side, this creates a BACK bet; for SELL, a LAY bet.
pub fn probability_to_bet(probability: Decimal, volume: Decimal, side: OrderSideSpecified) -> Bet {
    let price = Decimal::ONE / probability;
    match side {
        OrderSideSpecified::Buy => Bet::new(price, volume / price, BetSide::Back),
        OrderSideSpecified::Sell => Bet::new(price, volume / price, BetSide::Lay),
    }
}

/// Converts a probability and volume into a Bet using the inverse probability.
///
/// The side is also inverted (BUY becomes SELL and vice versa).
pub fn inverse_probability_to_bet(
    probability: Decimal,
    volume: Decimal,
    side: OrderSideSpecified,
) -> Bet {
    let inverse_probability = Decimal::ONE - probability;
    let inverse_side = match side {
        OrderSideSpecified::Buy => OrderSideSpecified::Sell,
        OrderSideSpecified::Sell => OrderSideSpecified::Buy,
    };
    probability_to_bet(inverse_probability, volume, inverse_side)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
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
        let formatted = format!("{}", bet);
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
        let formatted = format!("{}", position);

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
        assert!(position.exposure != dec!(0.0));
        position.reset();

        // After reset, the position should be cleared
        assert_eq!(position.price, dec!(0.0));
        assert_eq!(position.exposure, dec!(0.0));
        assert_eq!(position.realized_pnl, dec!(0.0));
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
        let bet = probability_to_bet(dec!(0.50), dec!(50.0), OrderSideSpecified::Buy);
        let expected = Bet::new(dec!(2.0), dec!(25.0), BetSide::Back);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec!(25.0));
        assert_eq!(bet.outcome_lose_payoff(), dec!(-25.0));
    }

    #[rstest]
    fn test_probability_to_bet_back_high_prob() {
        let bet = probability_to_bet(dec!(0.64), dec!(50.0), OrderSideSpecified::Buy);
        let expected = Bet::new(dec!(1.5625), dec!(32.0), BetSide::Back);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec!(18.0));
        assert_eq!(bet.outcome_lose_payoff(), dec!(-32.0));
    }

    #[rstest]
    fn test_probability_to_bet_back_low_prob() {
        let bet = probability_to_bet(dec!(0.40), dec!(50.0), OrderSideSpecified::Buy);
        let expected = Bet::new(dec!(2.5), dec!(20.0), BetSide::Back);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec!(30.0));
        assert_eq!(bet.outcome_lose_payoff(), dec!(-20.0));
    }

    #[rstest]
    fn test_probability_to_bet_sell() {
        let bet = probability_to_bet(dec!(0.80), dec!(50.0), OrderSideSpecified::Sell);
        let expected = Bet::new(dec_str("1.25"), dec_str("40"), BetSide::Lay);
        assert_eq!(bet, expected);
        assert_eq!(bet.outcome_win_payoff(), dec_str("-10"));
        assert_eq!(bet.outcome_lose_payoff(), dec_str("40"));
    }

    #[rstest]
    fn test_inverse_probability_to_bet() {
        // Original bet with SELL side
        let original_bet = probability_to_bet(dec!(0.80), dec!(100.0), OrderSideSpecified::Sell);
        // Equivalent reverse bet by buying the inverse probability
        let reverse_bet = probability_to_bet(dec!(0.20), dec!(100.0), OrderSideSpecified::Buy);
        let inverse_bet =
            inverse_probability_to_bet(dec!(0.80), dec!(100.0), OrderSideSpecified::Sell);

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
        let original_bet = probability_to_bet(dec!(0.64), dec!(50.0), OrderSideSpecified::Sell);
        let inverse_bet =
            inverse_probability_to_bet(dec!(0.64), dec!(50.0), OrderSideSpecified::Sell);

        assert_eq!(original_bet.stake, dec!(32.0));
        assert_eq!(original_bet.outcome_win_payoff(), dec!(-18.0));
        assert_eq!(original_bet.outcome_lose_payoff(), dec!(32.0));

        assert_eq!(inverse_bet.stake, dec!(18.0));
        assert_eq!(inverse_bet.outcome_win_payoff(), dec!(32.0));
        assert_eq!(inverse_bet.outcome_lose_payoff(), dec!(-18.0));
    }
}
