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

use clap::Parser;

#[derive(Debug, Parser)]
#[clap(version, about, author)]
pub struct NautilusCli {
    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Parser, Debug)]
pub enum Commands {
    Database(DatabaseOpt),
}

#[derive(Parser, Debug)]
#[command(about = "Postgres database operations", long_about = None)]
pub struct DatabaseOpt {
    #[clap(subcommand)]
    pub command: DatabaseCommand,
}

#[derive(Parser, Debug, Clone)]
pub struct DatabaseConfig {
    /// Hostname or IP address of the database server.
    #[arg(long)]
    pub host: Option<String>,
    /// Port number of the database server.
    #[arg(long)]
    pub port: Option<u16>,
    /// Username for connecting to the database.
    #[arg(long)]
    pub username: Option<String>,
    /// Name of the database.
    #[arg(long)]
    pub database: Option<String>,
    /// Password for connecting to the database.
    #[arg(long)]
    pub password: Option<String>,
    /// Directory path to the schema files.
    #[arg(long)]
    pub schema: Option<String>,
}

#[derive(Parser, Debug, Clone)]
#[command(about = "Postgres database operations", long_about = None)]
pub enum DatabaseCommand {
    /// Initializes a new Postgres database with the latest schema.
    Init(DatabaseConfig),
    /// Drops roles, privileges and deletes all data from the database.
    Drop(DatabaseConfig),
}
