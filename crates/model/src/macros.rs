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

//! Model specific macros.

#[macro_export]
macro_rules! enum_strum_serde {
    ($type:ty) => {
        impl $crate::__serde::Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>
            where
                S: $crate::__serde::Serializer,
            {
                serializer.serialize_str(&::std::string::ToString::to_string(self))
            }
        }

        impl<'de> $crate::__serde::Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
            where
                D: $crate::__serde::Deserializer<'de>,
            {
                let value =
                    <::std::borrow::Cow<'de, str> as $crate::__serde::Deserialize>::deserialize(
                        deserializer,
                    )?;
                <$type as ::core::str::FromStr>::from_str(value.as_ref())
                    .map_err($crate::__serde::de::Error::custom)
            }
        }
    };
}
