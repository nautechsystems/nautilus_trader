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

use nautilus_model::types::currency::Currency;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::sql::models::enums::CurrencyTypeModel;

pub struct CurrencyModel(pub Currency);

impl<'r> FromRow<'r, PgRow> for CurrencyModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id")?;
        let precision = row.try_get::<i32, _>("precision")?;
        let iso4217 = row.try_get::<i32, _>("iso4217")?;
        let name = row.try_get::<String, _>("name")?;
        let currency_type_model = row.try_get::<CurrencyTypeModel, _>("currency_type")?;
        let currency = Currency::new(
            id.as_str(),
            precision as u8,
            iso4217 as u16,
            name.as_str(),
            currency_type_model.0,
        );
        Ok(CurrencyModel(currency))
    }
}
