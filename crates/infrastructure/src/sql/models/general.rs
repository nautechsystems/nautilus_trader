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

use nautilus_model::identifiers::{ClientId, ClientOrderId};
use sqlx::{Error, FromRow, Row, postgres::PgRow};

#[derive(Debug, sqlx::FromRow)]
pub struct GeneralRow {
    pub id: String,
    pub value: Vec<u8>,
}

#[derive(Debug)]
pub struct OrderEventOrderClientIdCombination {
    pub client_order_id: ClientOrderId,
    pub client_id: ClientId,
}

impl<'r> FromRow<'r, PgRow> for OrderEventOrderClientIdCombination {
    fn from_row(row: &'r PgRow) -> Result<Self, Error> {
        let client_order_id = row
            .try_get::<&str, _>("client_order_id")
            .map(ClientOrderId::from)
            .unwrap();
        let client_id = row
            .try_get::<&str, _>("client_id")
            .map(ClientId::from)
            .unwrap();
        Ok(OrderEventOrderClientIdCombination {
            client_order_id,
            client_id,
        })
    }
}
