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

use anyhow::Context;
use bytes::Bytes;
use serde::{Serialize, de::DeserializeOwned};

use super::PayloadCodecError;

pub(super) fn deserialize<T>(payload: &[u8], type_name: &str) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    rmp_serde::from_slice(payload).with_context(|| format!("failed to decode MsgPack {type_name}"))
}

pub(super) fn serialize<T>(message: &T, type_name: &str) -> Result<Bytes, PayloadCodecError>
where
    T: Serialize,
{
    rmp_serde::to_vec_named(message)
        .map(Bytes::from)
        .map_err(|e| {
            PayloadCodecError::Failed(format!("MsgPack serialization failed for {type_name}: {e}"))
        })
}
