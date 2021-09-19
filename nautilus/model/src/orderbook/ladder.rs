// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::enums::OrderSide;
use crate::orderbook::level::Level;
use crate::orderbook::order::Order;
use min_max_heap::MinMaxHeap;

#[repr(C)]
#[derive(Debug)]
pub struct Ladder {
    pub side: OrderSide,
    pub levels: Box<MinMaxHeap<Level>>,
}

impl Ladder {
    pub fn new(side: OrderSide) -> Self {
        Ladder {
            side,
            levels: Box::new(MinMaxHeap::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.levels.len()
    }

    pub fn add(&mut self, order: Order) {
        match self.find_level_for_order(&order) {
            None => {
                self.levels.push(Level::from_order(order));
            }
            Some(mut level) => {
                level.add(order);
            }
        }
    }

    pub fn update(&mut self, order: Order) {
        match self.find_level_for_order(&order) {
            None => {
                self.levels.push(Level::from_order(order));
            }
            Some(mut level) => {
                level.update(order);
            }
        }
    }

    pub fn find_level_for_order(&mut self, order: &Order) -> Option<Level> {
        match self.side {
            OrderSide::Buy => self.levels.drain_desc().find(|l| l.price == order.price),
            OrderSide::Sell => self.levels.drain_asc().find(|l| l.price == order.price),
        }
    }
}
