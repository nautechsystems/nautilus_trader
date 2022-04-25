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
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Buffer8 {
    pub data: [u8; 8],
    pub len: u8,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Buffer16 {
    pub data: [u8; 16],
    pub len: u8,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Buffer32 {
    pub data: [u8; 32],
    pub len: u8,
}

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Buffer36 {
    pub data: [u8; 36],
    pub len: u8,
}
