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
    pub len: u8,
}

impl Buffer16 {
    pub fn from_str(s: &str) -> Buffer16 {
        assert!(s.is_ascii()); // Enforce ASCII only code points
        let mut buffer: [u8; 16] = [0; 16];
        let len = s.len() as u8;
        assert!(len <= 16);
        buffer[..len as usize].copy_from_slice(s.as_bytes());
        Buffer16 { data: buffer, len }
    }

    pub fn to_str(&self) -> String {
        String::from_utf8(self.data[..self.len as usize].to_vec()).unwrap()
    }
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Buffer32 {
    pub data: [u8; 32],
    pub len: u8,
}

impl Buffer32 {
    pub fn from_str(s: &str) -> Buffer32 {
        assert!(s.is_ascii()); // Enforce ASCII only code points
        let mut buffer: [u8; 32] = [0; 32];
        let len = s.len() as u8;
        assert!(len <= 32);
        buffer[..len as usize].copy_from_slice(s.as_bytes());
        Buffer32 { data: buffer, len }
    }

    pub fn to_str(&self) -> String {
        String::from_utf8(self.data[..self.len as usize].to_vec()).unwrap()
    }
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Buffer36 {
    pub data: [u8; 36],
    pub len: u8,
}

impl Buffer36 {
    pub fn from_str(s: &str) -> Buffer36 {
        assert!(s.is_ascii()); // Enforce ASCII only code points
        let mut buffer: [u8; 36] = [0; 36];
        let len = s.len() as u8;
        assert!(len <= 36);
        buffer[..len as usize].copy_from_slice(s.as_bytes());
        Buffer36 { data: buffer, len }
    }

    pub fn to_str(&self) -> String {
        String::from_utf8(self.data[..self.len as usize].to_vec()).unwrap()
    }
}

#[no_mangle]
pub extern "C" fn dummy_16(ptr: Buffer16) -> Buffer16 {
    ptr
}

#[no_mangle]
pub extern "C" fn dummy_32(ptr: Buffer32) -> Buffer32 {
    ptr
}
