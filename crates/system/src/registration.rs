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

use std::{any::type_name, fmt::Debug};

use nautilus_common::{
    component::Component,
    msgbus::{Endpoint, MStr},
};
use nautilus_model::identifiers::StrategyId;
use nautilus_trading::strategy::Strategy;

pub(crate) fn strategy_control_endpoint(strategy_id: StrategyId) -> MStr<Endpoint> {
    format!("{strategy_id}.control").into()
}

pub(crate) fn strategy_registration_id<T>(strategy: &T) -> String
where
    T: Strategy + Component + Debug + 'static,
{
    strategy.core().config.strategy_id.map_or_else(
        || {
            let strategy_type = type_name::<T>()
                .rsplit("::")
                .next()
                .unwrap_or_else(|| type_name::<T>());
            strategy_type.to_string()
        },
        |strategy_id| strategy_id.to_string(),
    )
}

pub(crate) fn base_strategy_id(strategy_id: &str) -> String {
    strategy_id
        .rsplit_once('-')
        .map_or_else(|| strategy_id.to_string(), |(base, _)| base.to_string())
}

pub(crate) fn ensure_unique_order_id_tag(
    existing_order_id_tags: &[&str],
    order_id_tag: &str,
) -> anyhow::Result<()> {
    if existing_order_id_tags.contains(&order_id_tag) {
        anyhow::bail!(
            "Strategy order_id_tag conflict for '{order_id_tag}', explicitly define unique order_id_tag values",
        );
    }

    Ok(())
}
