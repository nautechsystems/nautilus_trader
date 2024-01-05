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

use serde::{Deserialize, Serialize};

/// Represents types which are serializable for JSON and `MsgPack` specifications.
pub trait Serializable: Serialize + for<'de> Deserialize<'de> {
    /// Deserialize an object from JSON encoded bytes.
    fn from_json_bytes(data: Vec<u8>) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(&data)
    }

    /// Deserialize an object from `MsgPack` encoded bytes.
    fn from_msgpack_bytes(data: Vec<u8>) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(&data)
    }

    /// Serialize an object to JSON encoded bytes.
    fn as_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Serialize an object to `MsgPack` encoded bytes.
    fn as_msgpack_bytes(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec_named(self)
    }
}
