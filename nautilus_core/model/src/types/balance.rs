// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::{Display, Formatter};

use crate::{
    identifiers::instrument_id::InstrumentId,
    types::{currency::Currency, money::Money},
};

#[derive(Debug)]
pub struct AccountBalance {
    pub currency: Currency,
    pub total: Money,
    pub locked: Money,
    pub free: Money,
}

impl Display for AccountBalance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.currency.code, self.total, self.locked, self.free,
        )
    }
}

#[derive(Debug)]
pub struct MarginBalance {
    pub initial: Money,
    pub maintenance: Money,
    pub currency: Currency,
    pub instrument_id: InstrumentId,
}

impl Display for MarginBalance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.currency.code, self.initial, self.maintenance, self.instrument_id,
        )
    }
}
