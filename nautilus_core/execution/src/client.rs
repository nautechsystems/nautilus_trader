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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use nautilus_common::cache::Cache;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{account_id::AccountId, venue::Venue},
    types::currency::Currency,
};

pub struct ExecutionClient {
    pub venue: Venue,
    pub oms_type: OmsType,
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub is_connected: bool,
    cache: &'static Cache,
}

impl ExecutionClient {
    // pub fn get_account(&self) -> Box<dyn Account> {
    //     todo!();
    // }

    // -- COMMAND HANDLERS ----------------------------------------------------

    // pub fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()> {
    //     todo!();
    // }
}
