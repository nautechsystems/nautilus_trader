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

use anyhow::Result;
use dotenv::dotenv;
use nautilus_persistence::db::database::{init_db_schema, Database};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // load envs if exists
    dotenv().ok();
    let db = Database::new(None, None).await;
    let sql_schema_dir = "../schema/sql";
    init_db_schema(&db, sql_schema_dir).await?;
    Ok(())
}
