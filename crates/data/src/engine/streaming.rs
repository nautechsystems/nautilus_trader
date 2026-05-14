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

use ahash::AHashMap;
use nautilus_common::messages::data::{
    SubscribeBars, SubscribeCommand, SubscribeQuotes, SubscribeTrades,
};
use nautilus_core::{
    Params,
    correctness::{FAILED, check_key_not_in_map},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use serde_json::Value;
use ustr::Ustr;

use super::DataEngine;

pub(crate) type CatalogMap = AHashMap<Ustr, ParquetDataCatalog>;

impl DataEngine {
    /// Registers the `catalog` with the engine with an optional specific `name`.
    ///
    /// # Panics
    ///
    /// Panics if a catalog with the same `name` has already been registered.
    pub fn register_catalog(&mut self, catalog: ParquetDataCatalog, name: Option<&str>) {
        let name = Ustr::from(name.unwrap_or("catalog_0"));

        check_key_not_in_map(&name, &self.catalogs, "name", "catalogs").expect(FAILED);

        self.catalogs.insert(name, catalog);
        log::info!("Registered catalog <{name}>");
    }

    pub(super) fn subscribe_command_with_prefilled_start_ns(
        &self,
        cmd: SubscribeCommand,
    ) -> anyhow::Result<SubscribeCommand> {
        match cmd {
            SubscribeCommand::Quotes(cmd) if Self::is_start_ns_missing(cmd.params.as_ref()) => {
                let identifier = cmd.instrument_id.to_string();
                let params = self.params_with_prefilled_start_ns(
                    cmd.params.as_ref(),
                    "quotes",
                    &identifier,
                )?;
                Ok(SubscribeCommand::Quotes(SubscribeQuotes { params, ..cmd }))
            }
            SubscribeCommand::Trades(cmd) if Self::is_start_ns_missing(cmd.params.as_ref()) => {
                let identifier = cmd.instrument_id.to_string();
                let params = self.params_with_prefilled_start_ns(
                    cmd.params.as_ref(),
                    "trades",
                    &identifier,
                )?;
                Ok(SubscribeCommand::Trades(SubscribeTrades { params, ..cmd }))
            }
            SubscribeCommand::Bars(cmd)
                if cmd.bar_type.is_externally_aggregated()
                    && Self::is_start_ns_missing(cmd.params.as_ref()) =>
            {
                let identifier = cmd.bar_type.to_string();
                let params =
                    self.params_with_prefilled_start_ns(cmd.params.as_ref(), "bars", &identifier)?;
                Ok(SubscribeCommand::Bars(SubscribeBars { params, ..cmd }))
            }
            _ => Ok(cmd),
        }
    }

    fn is_start_ns_missing(params: Option<&Params>) -> bool {
        params.is_none_or(|params| !params.contains_key("start_ns"))
    }

    fn params_with_prefilled_start_ns(
        &self,
        params: Option<&Params>,
        data_cls: &str,
        identifier: &str,
    ) -> anyhow::Result<Option<Params>> {
        let start_ns = self
            .catalog_last_timestamp(data_cls, identifier)?
            .map_or(Value::Null, |last_timestamp| {
                Value::from(last_timestamp.saturating_add(1))
            });
        let mut params = params.cloned().unwrap_or_else(Params::new);

        params.insert("start_ns".to_string(), start_ns);

        Ok(Some(params))
    }

    fn catalog_last_timestamp(
        &self,
        data_cls: &str,
        identifier: &str,
    ) -> anyhow::Result<Option<u64>> {
        for catalog in self.catalogs.values() {
            if let Some(last_timestamp) =
                catalog.query_last_timestamp(data_cls, Some(identifier))?
            {
                return Ok(Some(last_timestamp));
            }
        }

        Ok(None)
    }
}
