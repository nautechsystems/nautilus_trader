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

macro_rules! impl_serialization_for_identifier {
    ($ty:ty) => {
        impl Serialize for $ty {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                self.value.serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value_str: &str = Deserialize::deserialize(deserializer)?;
                let value: $ty = FromStr::from_str(value_str).map_err(serde::de::Error::custom)?;
                Ok(value)
            }
        }
    };
}

macro_rules! impl_from_str_for_identifier {
    ($ty:ty) => {
        impl FromStr for $ty {
            type Err = String;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                Self::new(input).map_err(|e| e.to_string())
            }
        }
    };
}

#[cfg(feature = "python")]
macro_rules! identifier_for_python {
    ($ty:ty) => {
        #[pymethods]
        impl $ty {
            #[new]
            fn py_new(value: &str) -> PyResult<Self> {
                match <$ty>::new(value) {
                    Ok(instance) => Ok(instance),
                    Err(e) => Err(to_pyvalue_err(e)),
                }
            }

            fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
                match op {
                    CompareOp::Eq => self.eq(other).into_py(py),
                    CompareOp::Ne => self.ne(other).into_py(py),
                    _ => py.NotImplemented(),
                }
            }

            fn __hash__(&self) -> isize {
                self.value.precomputed_hash() as isize
            }

            fn __str__(&self) -> &'static str {
                self.value.as_str()
            }

            fn __repr__(&self) -> String {
                format!("{}('{}')", stringify!($ty), self.value)
            }

            #[getter]
            #[pyo3(name = "value")]
            fn py_value(&self) -> String {
                self.value.to_string()
            }
        }
    };
}
