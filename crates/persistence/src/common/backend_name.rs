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

//! Shared catalog and writer backend-name enum generation.

macro_rules! backend_type {
    (
        $(#[$meta:meta])*
        $name:ident {
            $default:ident => $default_name:expr,
            $($variant:ident => $variant_name:expr),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
        pub enum $name {
            #[default]
            $default,
            $($variant,)*
            External(String),
        }

        impl Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::$default => f.write_str($default_name),
                    $(Self::$variant => f.write_str($variant_name),)*
                    Self::External(name) => f.write_str(name),
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = anyhow::Error;

            fn from_str(value: &str) -> anyhow::Result<Self> {
                if value.is_empty() {
                    anyhow::bail!("Invalid empty `{}`", stringify!($name))
                } else if value.eq_ignore_ascii_case($default_name) {
                    Ok(Self::$default)
                } $(else if value.eq_ignore_ascii_case($variant_name) {
                    Ok(Self::$variant)
                })* else {
                    Ok(Self::External(value.to_string()))
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                let name = <String as serde::Deserialize>::deserialize(deserializer)?;
                name.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use backend_type;
