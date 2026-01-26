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

use clap::Parser;
use nautilus_cli::opt::NautilusCli;
use nautilus_common::logging::ensure_logging_initialized;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    ensure_logging_initialized();

    if let Err(e) = nautilus_cli::run(NautilusCli::parse()).await {
        log::error!("Error executing Nautilus CLI: {e}");
    }
}
