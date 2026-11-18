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

use std::{fs, path::PathBuf};

pub(crate) fn load_test_json(filename: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(filename);
    let content = fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

/// Returns an account balance response with one detail per `(ccy, cashBal, liab)`.
pub(crate) fn balance_response(details: &[(&str, &str, &str)]) -> serde_json::Value {
    let mut response = load_test_json("http_get_account_balance.json");
    let template = response["data"][0]["details"][0].clone();
    response["data"][0]["details"] = details
        .iter()
        .map(|(ccy, cash_bal, liab)| {
            let mut detail = template.clone();
            detail["ccy"] = (*ccy).into();
            detail["cashBal"] = (*cash_bal).into();
            detail["liab"] = (*liab).into();
            detail
        })
        .collect();

    response
}
