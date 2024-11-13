// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! Provides a generic `Portfolio` for all environments.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use nautilus_analysis::analyzer::PortfolioAnalyzer;
use nautilus_common::{cache::Cache, clock::Clock, msgbus::MessageBus};
use nautilus_model::{
    accounts::any::AccountAny,
    data::quote::QuoteTick,
    enums::OrderSide,
    events::{account::state::AccountState, order::OrderEventAny, position::PositionEvent},
    identifiers::{InstrumentId, Venue},
    instruments::any::InstrumentAny,
    position::Position,
    types::money::Money,
};
use rust_decimal::Decimal;

pub struct Portfolio {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    accounts: HashMap<Venue, AccountAny>,
    analyzer: PortfolioAnalyzer,
    // venue: Option<Venue>, // Added for completeness but was meant to be a "temporary hack"
    unrealized_pnls: HashMap<InstrumentId, Money>,
    realized_pnls: HashMap<InstrumentId, Money>,
    net_positions: HashMap<InstrumentId, Decimal>,
    pending_calcs: HashSet<InstrumentId>,
    initialized: bool,
}

impl Portfolio {
    // pub fn set_specific_venue(&mut self, venue: Venue) { // Lets try not to use this?
    //     todo!()
    // }

    // -- QUERIES ---------------------------------------------------------------------------------

    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }

    #[must_use]
    pub const fn analyzer(&self) -> &PortfolioAnalyzer {
        &self.analyzer
    }

    #[must_use]
    pub fn account(&self, venue: &Venue) -> Option<&AccountAny> {
        self.accounts.get(venue)
    }

    #[must_use]
    pub fn balances_locked(&self, venue: &Venue) -> HashMap<Venue, Money> {
        todo!()
    }

    #[must_use]
    pub fn margins_init(&self, venue: &Venue) -> HashMap<Venue, Money> {
        todo!()
    }

    #[must_use]
    pub fn margins_maint(&self, venue: &Venue) -> HashMap<Venue, Money> {
        todo!()
    }

    #[must_use]
    pub fn unrealized_pnls(&self, venue: &Venue) -> HashMap<Venue, Money> {
        todo!()
    }

    #[must_use]
    pub fn realized_pnls(&self, venue: &Venue) -> HashMap<Venue, Money> {
        todo!()
    }

    #[must_use]
    pub fn net_exposures(&self, venue: &Venue) -> HashMap<Venue, Money> {
        todo!()
    }

    #[must_use]
    pub fn unrealized_pnl(&self, instrument_id: &InstrumentId) -> Option<Money> {
        todo!()
    }

    #[must_use]
    pub fn realized_pnl(&self, instrument_id: &InstrumentId) -> Option<Money> {
        todo!()
    }

    #[must_use]
    pub fn net_exposure(&self, instrument_id: &InstrumentId) -> Option<Money> {
        todo!()
    }

    #[must_use]
    pub fn net_position(&self, instrument_id: &InstrumentId) -> Option<Decimal> {
        todo!()
    }

    #[must_use]
    pub fn is_net_long(&self, instrument_id: &InstrumentId) -> bool {
        todo!()
    }

    #[must_use]
    pub fn is_net_short(&self, instrument_id: &InstrumentId) -> bool {
        todo!()
    }

    #[must_use]
    pub fn is_flat(&self, instrument_id: &InstrumentId) -> bool {
        todo!()
    }

    #[must_use]
    pub fn is_completely_flat(&self) -> bool {
        todo!()
    }

    // -- COMMANDS --------------------------------------------------------------------------------

    pub fn initialize_orders(&mut self) {
        todo!()
    }

    pub fn initialize_positions(&mut self) {
        todo!()
    }

    pub fn update_quote_tick(&mut self, quote: &QuoteTick) {
        todo!()
    }

    pub fn update_account(&mut self, event: &AccountState) {
        todo!()
    }

    pub fn update_order(&mut self, event: &OrderEventAny) {
        todo!()
    }

    pub fn update_position(&mut self, event: &PositionEvent) {
        todo!()
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    // fn net_position(&self, instrument_id: &InstrumentId) -> Decimal {  // Same as above?
    //     todo!()
    // }

    fn update_net_position(
        &self,
        instrument_id: &InstrumentId,
        positions_open: Vec<&Position>,
    ) -> Decimal {
        todo!()
    }

    fn calculate_unrealized_pnl(&self, instrument_id: &InstrumentId) -> Money {
        todo!()
    }

    fn calculate_realized_pnl(&self, instrument_id: &InstrumentId) -> Money {
        todo!()
    }

    fn get_last_price(&self, position: &Position) -> Money {
        todo!()
    }

    fn calculate_xrate_to_base(
        &self,
        account: &AccountAny,
        instrument: &InstrumentAny,
        side: OrderSide,
    ) -> f64 {
        todo!()
    }
}
