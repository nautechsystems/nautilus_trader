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

use crate::common::error::BulletError;

/// Try to parse an API error JSON body from a non-2xx response.
pub fn parse_api_error(body: &str, http_status: u16) -> BulletError {
    match serde_json::from_str::<crate::common::models::ApiErrorResponse>(body) {
        Ok(api_err) => {
            // 401 with "Invalid signature" means schema rotation
            if http_status == 401 && api_err.message.to_lowercase().contains("invalid signature") {
                return BulletError::TransactionOutdated;
            }
            BulletError::Api { status: api_err.status, message: api_err.message }
        }
        Err(_) => BulletError::Http(format!("HTTP {http_status}: {body}")),
    }
}
