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

use std::str::FromStr;

use anyhow::Context;
use nautilus_common::messages::data::RequestCommand;
use nautilus_core::{Params, UUID4};
use nautilus_model::data::BarType;

#[derive(Debug, Clone)]
pub(super) struct RequestBarAggregation {
    pub(super) bar_types: Vec<BarType>,
    pub(super) update_subscriptions: bool,
}

impl RequestBarAggregation {
    pub(super) const fn aggregator_request_id(&self, request_id: UUID4) -> Option<UUID4> {
        if self.update_subscriptions {
            None
        } else {
            Some(request_id)
        }
    }
}

pub(super) fn request_params(req: &RequestCommand) -> Option<&Params> {
    match req {
        RequestCommand::Data(cmd) => cmd.params.as_ref(),
        RequestCommand::Instrument(cmd) => cmd.params.as_ref(),
        RequestCommand::Instruments(cmd) => cmd.params.as_ref(),
        RequestCommand::BookSnapshot(cmd) => cmd.params.as_ref(),
        RequestCommand::BookDepth(cmd) => cmd.params.as_ref(),
        RequestCommand::Quotes(cmd) => cmd.params.as_ref(),
        RequestCommand::Trades(cmd) => cmd.params.as_ref(),
        RequestCommand::FundingRates(cmd) => cmd.params.as_ref(),
        RequestCommand::ForwardPrices(cmd) => cmd.params.as_ref(),
        RequestCommand::Bars(cmd) => cmd.params.as_ref(),
    }
}

pub(super) fn request_bar_aggregation_from_params(
    params: Option<&Params>,
) -> anyhow::Result<Option<RequestBarAggregation>> {
    let Some(params) = params else {
        return Ok(None);
    };
    let Some(value) = params.get("bar_types") else {
        return Ok(None);
    };

    let values = value
        .as_array()
        .context("`bar_types` request parameter must be an array")?;
    let mut bar_types = Vec::with_capacity(values.len());
    for value in values {
        let raw = value
            .as_str()
            .context("`bar_types` request parameter must contain strings")?;
        bar_types
            .push(BarType::from_str(raw).context("failed to parse `bar_types` request parameter")?);
    }

    if bar_types.is_empty() {
        return Ok(None);
    }

    if let Some(bar_type) = bar_types
        .iter()
        .find(|bar_type| !bar_type.is_internally_aggregated())
    {
        anyhow::bail!("Cannot request aggregated bars: {bar_type} must be internally aggregated");
    }

    let mut unique_bar_types = Vec::with_capacity(bar_types.len());
    for bar_type in bar_types {
        if !unique_bar_types.contains(&bar_type) {
            unique_bar_types.push(bar_type);
        }
    }

    let update_subscriptions = params.get_bool("update_subscriptions").unwrap_or(false);

    Ok(Some(RequestBarAggregation {
        bar_types: unique_bar_types,
        update_subscriptions,
    }))
}
