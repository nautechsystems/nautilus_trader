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

use dotenv::dotenv;
use nautilus_persistence::db::database::{Database, DatabaseEngine};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // load envs if exists
    dotenv().ok();

    let db = Database::new(Some(DatabaseEngine::POSTGRES), None).await;

    db.execute("DROP SCHEMA IF EXISTS nautilus CASCADE;")
        .await
        .map_err(|e| e.to_string())?;
    db.execute("DROP ROLE IF EXISTS nautilus;")
        .await
        .map_err(|e| e.to_string())?;
    println!("Dropped nautilus schema and role.");
    Ok(())
}
