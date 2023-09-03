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

use std::{
    collections::hash_map::DefaultHasher,
    ffi::{c_char, CStr, CString},
    fmt::{Debug, Display, Formatter},
    hash::{Hash, Hasher},
    str::FromStr,
};

use pyo3::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::python::to_pyvalue_err;

#[repr(C)]
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
#[pyclass]
pub struct UUID4 {
    value: [u8; 37],
}

impl UUID4 {
    #[must_use]
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        let c_string = CString::new(uuid.to_string()).expect("`CString` conversion failed");
        let bytes = c_string.as_bytes_with_nul();
        let mut value = [0; 37];
        value[..bytes.len()].copy_from_slice(bytes);

        Self { value }
    }

    #[must_use]
    pub fn to_cstr(&self) -> &CStr {
        // Safety: unwrap is safe here as we always store valid C strings
        CStr::from_bytes_with_nul(&self.value).unwrap()
    }
}

impl FromStr for UUID4 {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(s).map_err(|_| "Invalid UUID string")?;
        let c_string = CString::new(uuid.to_string()).expect("`CString` conversion failed");
        let bytes = c_string.as_bytes_with_nul();
        let mut value = [0; 37];
        value[..bytes.len()].copy_from_slice(bytes);

        Ok(Self { value })
    }
}

impl From<&str> for UUID4 {
    fn from(input: &str) -> Self {
        input.parse().unwrap_or_else(|err| panic!("{}", err))
    }
}

impl Default for UUID4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_cstr().to_string_lossy())
    }
}

impl Serialize for UUID4 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UUID4 {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let uuid4_str: &str = Deserialize::deserialize(_deserializer)?;
        let uuid4: Self = uuid4_str.into();
        Ok(uuid4)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl UUID4 {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> String {
        self.to_string()
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn uuid4_new() -> UUID4 {
    UUID4::new()
}

/// Returns a [`UUID4`] from C string pointer.
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
///
/// # Panics
///
/// - If `ptr` cannot be cast to a valid C string.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn uuid4_from_cstr(ptr: *const c_char) -> UUID4 {
    assert!(!ptr.is_null(), "`ptr` was NULL");
    UUID4::from(
        CStr::from_ptr(ptr)
            .to_str()
            .unwrap_or_else(|_| panic!("CStr::from_ptr failed")),
    )
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn uuid4_to_cstr(uuid: &UUID4) -> *const c_char {
    uuid.to_cstr().as_ptr()
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn uuid4_eq(lhs: &UUID4, rhs: &UUID4) -> u8 {
    u8::from(lhs == rhs)
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn uuid4_hash(uuid: &UUID4) -> u64 {
    let mut h = DefaultHasher::new();
    uuid.hash(&mut h);
    h.finish()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use rstest::*;
    use uuid;

    use super::*;

    #[rstest]
    fn test_uuid4_new() {
        let uuid = UUID4::new();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
        assert_eq!(uuid_parsed.to_string().len(), 36);
    }

    #[rstest]
    fn test_uuid4_default() {
        let uuid: UUID4 = UUID4::default();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
    }

    #[rstest]
    fn test_uuid4_from_str() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        let result_string = uuid.to_string();
        let result_parsed = Uuid::parse_str(&result_string).expect("Uuid::parse_str failed");
        let expected_parsed = Uuid::parse_str(uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(result_parsed, expected_parsed);
    }

    #[rstest]
    fn test_equality() {
        let uuid1 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid2 = UUID4::from("46922ecb-4324-4e40-a56c-841e0d774cef");
        assert_eq!(uuid1, uuid1);
        assert_ne!(uuid1, uuid2);
    }

    #[rstest]
    fn test_uuid4_display() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        let result_string = format!("{uuid}");
        assert_eq!(result_string, uuid_string);
    }

    #[rstest]
    fn test_c_api_uuid4_new() {
        let uuid = uuid4_new();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).expect("Uuid::parse_str failed");
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
    }

    #[rstest]
    fn test_c_api_uuid4_from_cstr() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid_cstring = CString::new(uuid_string).expect("CString::new failed");
        let uuid_ptr = uuid_cstring.as_ptr();
        let uuid = unsafe { uuid4_from_cstr(uuid_ptr) };
        assert_eq!(uuid_string, uuid.to_string());
    }

    #[rstest]
    fn test_c_api_uuid4_to_cstr() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        let uuid = UUID4::from(uuid_string);
        let uuid_ptr = uuid4_to_cstr(&uuid);
        let uuid_cstr = unsafe { CStr::from_ptr(uuid_ptr) };
        let uuid_result_string = uuid_cstr.to_str().expect("CStr::to_str failed").to_string();
        assert_eq!(uuid_string, uuid_result_string);
    }

    #[rstest]
    fn test_c_api_uuid4_eq() {
        let uuid1 = UUID4::from("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
        let uuid2 = UUID4::from("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
        let uuid3 = UUID4::from("6ba7b810-9dad-11d1-80b4-00c04fd430c9");
        assert_eq!(uuid4_eq(&uuid1, &uuid2), 1);
        assert_eq!(uuid4_eq(&uuid1, &uuid3), 0);
    }

    #[rstest]
    fn test_c_api_uuid4_hash() {
        let uuid1 = UUID4::from("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
        let uuid2 = UUID4::from("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
        let uuid3 = UUID4::from("6ba7b810-9dad-11d1-80b4-00c04fd430c9");
        assert_eq!(uuid4_hash(&uuid1), uuid4_hash(&uuid2));
        assert_ne!(uuid4_hash(&uuid1), uuid4_hash(&uuid3));
    }
}
