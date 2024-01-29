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

use databento::dbn;
use pyo3::prelude::*;
use serde::Deserialize;
use ustr::Ustr;

/// Represents a Databento publisher ID.
pub type PublisherId = u16;

/// Represents a Databento dataset code.
pub type Dataset = Ustr;

#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct DatabentoPublisher {
    pub publisher_id: PublisherId,
    pub dataset: dbn::Dataset,
    pub venue: dbn::Venue,
    pub description: String,
}
