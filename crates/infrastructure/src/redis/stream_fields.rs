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

use nautilus_common::msgbus::BusMessage;

pub(crate) const PAYLOAD_KIND_FIELD: &str = "payload_kind";
pub(crate) const PAYLOAD_KIND_TYPED: &str = "typed";
const FIELD_COUNT_BASE: usize = 4;
const FIELD_COUNT_TYPED: usize = 5;

type BusMessageField<'a> = (&'static str, &'a [u8]);

pub(crate) struct BusMessageFields<'a> {
    fields: [BusMessageField<'a>; FIELD_COUNT_TYPED],
    len: usize,
}

impl<'a> BusMessageFields<'a> {
    pub(crate) fn as_slice(&self) -> &[BusMessageField<'a>] {
        &self.fields[..self.len]
    }
}

pub(crate) fn bus_message_fields<'a>(
    message: &'a BusMessage,
    encoding: &'a str,
) -> BusMessageFields<'a> {
    let len = if message.payload_type.is_typed_message() {
        FIELD_COUNT_TYPED
    } else {
        FIELD_COUNT_BASE
    };

    BusMessageFields {
        fields: [
            ("topic", message.topic.as_ref()),
            ("type", message.payload_type.as_str().as_bytes()),
            ("payload", message.payload.as_ref()),
            ("encoding", encoding.as_bytes()),
            (PAYLOAD_KIND_FIELD, PAYLOAD_KIND_TYPED.as_bytes()),
        ],
        len,
    }
}
