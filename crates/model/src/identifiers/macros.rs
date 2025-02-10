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

//! Provides macros for generating identifier functionality.

macro_rules! impl_serialization_for_identifier {
    ($ty:ty) => {
        impl Serialize for $ty {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                self.inner().serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value_str: &str = Deserialize::deserialize(deserializer)?;
                let value: $ty = value_str.into();
                Ok(value)
            }
        }
    };
}

macro_rules! impl_from_str_for_identifier {
    ($ty:ty) => {
        impl From<&str> for $ty {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $ty {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }
    };
}

macro_rules! impl_as_ref_for_identifier {
    ($ty:ty) => {
        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    };
}
