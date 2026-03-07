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

//! Typed handlers for routing market data events to the option chain manager.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{msgbus::Handler, timer::TimeEvent};
use nautilus_core::WeakCell;
use nautilus_model::{
    data::{QuoteTick, option_chain::OptionGreeks},
    identifiers::OptionSeriesId,
};
use ustr::Ustr;

use super::OptionChainManager;

/// Routes incoming quote ticks to the `OptionChainManager` for aggregation.
///
/// Follows the same `WeakCell` pattern as `BarQuoteHandler`.
#[derive(Debug)]
pub struct OptionChainQuoteHandler {
    manager: WeakCell<OptionChainManager>,
    id: Ustr,
}

impl OptionChainQuoteHandler {
    pub fn new(manager: Rc<RefCell<OptionChainManager>>, series_id: OptionSeriesId) -> Self {
        let id = Ustr::from(&format!("OptionChainQuoteHandler({series_id})"));
        Self {
            manager: WeakCell::from(Rc::downgrade(&manager)),
            id,
        }
    }
}

impl Handler<QuoteTick> for OptionChainQuoteHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, quote: &QuoteTick) {
        if let Some(mgr) = self.manager.upgrade() {
            mgr.borrow_mut().handle_quote(quote);
        }
    }
}

/// Routes incoming option greeks to the `OptionChainManager` for aggregation.
#[derive(Debug)]
pub struct OptionChainGreeksHandler {
    manager: WeakCell<OptionChainManager>,
    id: Ustr,
}

impl OptionChainGreeksHandler {
    pub fn new(manager: Rc<RefCell<OptionChainManager>>, series_id: OptionSeriesId) -> Self {
        let id = Ustr::from(&format!("OptionChainGreeksHandler({series_id})"));
        Self {
            manager: WeakCell::from(Rc::downgrade(&manager)),
            id,
        }
    }
}

impl Handler<OptionGreeks> for OptionChainGreeksHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, greeks: &OptionGreeks) {
        if let Some(mgr) = self.manager.upgrade() {
            mgr.borrow_mut().handle_greeks(greeks);
        }
    }
}

/// Timer callback that triggers snapshot publishing for a per-series manager.
///
/// Follows the same closure-based timer pattern as `BookSnapshotter`.
#[derive(Debug)]
pub struct OptionChainSlicePublisher {
    manager: WeakCell<OptionChainManager>,
}

impl OptionChainSlicePublisher {
    pub fn new(manager: Rc<RefCell<OptionChainManager>>) -> Self {
        Self {
            manager: WeakCell::from(Rc::downgrade(&manager)),
        }
    }

    /// Called by the timer — takes the accumulated snapshot and publishes it.
    pub fn publish(&self, event: TimeEvent) {
        if let Some(mgr) = self.manager.upgrade() {
            mgr.borrow_mut().publish_slice(event.ts_event);
        }
    }
}
