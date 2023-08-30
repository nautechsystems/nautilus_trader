// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

#[macro_export]
macro_rules! enum_strum_serde {
    ($type:ty) => {
        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let s = String::deserialize(deserializer)?;
                <$type>::from_str(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

#[cfg(feature = "python")]
#[macro_export]
macro_rules! enum_for_python {
    ($type:ty) => {
        #[pymethods]
        impl $type {
            #[new]
            fn py_new(py: Python<'_>, value: &PyAny) -> PyResult<Self> {
                let t = Self::type_object(py);
                Self::py_from_str(t, value)
            }

            fn __hash__(&self) -> isize {
                *self as isize
            }

            fn __str__(&self) -> String {
                self.to_string()
            }

            fn __repr__(&self) -> String {
                format!(
                    "<{}.{}: '{}'>",
                    stringify!($type),
                    self.name(),
                    self.value(),
                )
            }

            #[getter]
            pub fn name(&self) -> String {
                self.to_string()
            }

            #[getter]
            pub fn value(&self) -> u8 {
                *self as u8
            }

            #[classmethod]
            fn variants(_: &PyType, py: Python<'_>) -> EnumIterator {
                EnumIterator::new::<Self>(py)
            }

            #[classmethod]
            #[pyo3(name = "from_str")]
            fn py_from_str(_: &PyType, data: &PyAny) -> PyResult<Self> {
                let data_str: &str = data.str().and_then(|s| s.extract())?;
                let tokenized = data_str.to_uppercase();
                Self::from_str(&tokenized).map_err(|e| PyValueError::new_err(format!("{e:?}")))
            }
        }
    };
}
