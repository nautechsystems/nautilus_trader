// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Buffer16 {
    pub data: [u8; 16],
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Buffer32 {
    pub data: [u8; 32],
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Buffer36 {
    pub data: [u8; 36],
    pub len: usize,
}

pub trait Buffer {
    fn to_str(&self) -> String;
    fn len(&self) -> usize;
    fn capacity(&self) -> usize;
}

macro_rules! impl_buffer_trait {
    ($name:ident, $size:literal) => {

        impl From<&str> for $name {
            fn from(s: &str) -> $name {
                assert!(s.is_ascii()); // Enforce ASCII only code points
                let mut buffer: [u8; $size] = [0; $size];
                let len = s.len();
                assert!(len <= $size);
                buffer[..len].copy_from_slice(s.as_bytes());
                $name { data: buffer, len }
            }
        }

        impl Buffer for $name {
            fn to_str(&self) -> String {
                String::from_utf8(self.data[..self.len].to_vec()).unwrap()
            }
            fn len(&self) -> usize {
                self.len
            }
            fn capacity(&self) -> usize {
                $size
            }
        }
    };
}

impl_buffer_trait!(Buffer16, 16);
impl_buffer_trait!(Buffer32, 32);
impl_buffer_trait!(Buffer36, 36);

#[no_mangle]
pub extern "C" fn dummy_16(ptr: Buffer16) -> Buffer16 {
    ptr
}

#[no_mangle]
pub extern "C" fn dummy_32(ptr: Buffer32) -> Buffer32 {
    ptr
}

#[cfg(test)]
mod tests {
    use super::{Buffer, Buffer16};

    #[test]
    fn test_len() {
        let b = Buffer16::from("");
        let b1 = Buffer16::from("testing testing");
        assert_eq!(b.len(), 0);
        assert_eq!(b1.len(), 15);
    }

    #[test]
    fn test_identity() {
        let b = Buffer16::from("");
        let b1 = Buffer16::from("testing testing");
        assert_eq!(b.to_str(), "");
        assert_eq!(b1.to_str(), "testing testing");
    }

    #[test]
    fn test_panic() {
        let result = std::panic::catch_unwind(|| Buffer16::from("more testing will panic"));
        let result1 = std::panic::catch_unwind(|| Buffer16::from("😜️💥💀"));
        assert!(result.is_err());
        assert!(result1.is_err());
    }
}
