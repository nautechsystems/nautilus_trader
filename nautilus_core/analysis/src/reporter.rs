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

use std::collections::HashMap;

use nautilus_model::{accounts::base::Account, orders::base::Order, position::Position};

pub struct ReportProvider {}

impl ReportProvider {
    // pub fn generate_orders_report(orders: Vec<Box<dyn Order>>) -> Vec<f64> {

    // }

    pub fn generate_orders_report(orders: Vec<Box<dyn Order>>) -> Vec<HashMap<String, String>> {
        // if orders.is_empty() {
            return Vec::new();
        // }

        // // Convert orders to a list of dictionaries
        // let mut orders_all: Vec<HashMap<String, String>> =
        //     orders.iter().map(|o| o.to_dict()).collect();

        // // Sort by "client_order_id"
        // orders_all.sort_by(|a, b| {
        //     a.get("client_order_id")
        //         .unwrap_or(&String::new())
        //         .cmp(b.get("client_order_id").unwrap_or(&String::new()))
        // });

        // orders_all
    }

    pub fn generate_order_fills_report(orders: Vec<Box<dyn Order>>) {

    }

    pub fn generate_fills_report(orders: Vec<Box<dyn Order>>) {}

    pub fn generate_positions_report(orders: &[Position]) {}

    pub fn generate_account_report(account: Box<dyn Account>) {
        let mut balances: HashMap<String, String> = HashMap::new();
        for state in account.events().iter() {
            // if let Some(state_balances) = state.get("balances") {
            //     for balance in state_balances.iter() {
            //         let mut combined = balance.clone();
            //         // Merge balance and state data (in this case, just the balances for simplicity)
            //         combined.extend(balance.clone());
            //         balances.push(combined);
            //     }
            // }
        }

        // pub account_id: AccountId,
        // pub account_type: AccountType,
        // pub base_currency: Option<Currency>,
        // pub balances: Vec<AccountBalance>,
        // pub margins: Vec<MarginBalance>,
        // pub is_reported: bool,
        // pub event_id: UUID4,
        // pub ts_event: UnixNanos,
        // pub ts_init: UnixNanos,




    }
}
