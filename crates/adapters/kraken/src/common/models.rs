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

//! Common data structures shared across the Kraken adapter.

use serde::{Deserialize, Serialize};

/// Generic Kraken API response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenResponse<T> {
    pub result: Option<T>,
    pub error: Option<Vec<String>>,
    #[serde(default)]
    pub success: bool,
}

impl<T> KrakenResponse<T> {
    /// Returns true if the response indicates success.
    pub fn is_success(&self) -> bool {
        self.success || (self.error.is_none() || self.error.as_ref().is_some_and(|e| e.is_empty()))
    }

    /// Returns the error message if present.
    pub fn error_message(&self) -> Option<String> {
        self.error
            .as_ref()
            .filter(|e| !e.is_empty())
            .map(|e| e.join(", "))
    }
}
