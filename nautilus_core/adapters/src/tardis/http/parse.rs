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

use nautilus_model::instruments::any::InstrumentAny;

use crate::tardis::enums::InstrumentType;

use super::types::InstrumentInfo;

pub fn parse_instrument_any(info: InstrumentInfo) -> InstrumentAny {
    match info.instrument_type {
        InstrumentType::Spot => parse_spot_instrument(info),
        InstrumentType::Perpetual => parse_perp_instrument(info),
        InstrumentType::Future => parse_future_instrument(info),
        InstrumentType::Option => parse_option_instrument(info),
    }
}

fn parse_spot_instrument(_info: InstrumentInfo) -> InstrumentAny {
    todo!()
}

fn parse_perp_instrument(_info: InstrumentInfo) -> InstrumentAny {
    todo!()
}

fn parse_future_instrument(_info: InstrumentInfo) -> InstrumentAny {
    todo!()
}

fn parse_option_instrument(_info: InstrumentInfo) -> InstrumentAny {
    todo!()
}
