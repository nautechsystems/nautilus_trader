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

//! Blockchain event data structures and parsers.
//!
//! This module provides types and utilities for parsing and handling various blockchain events
//! emitted by smart contracts, particularly DeFi protocol events such as swaps, mints, burns,
//! and pool creation events.

pub mod burn;
pub mod collect;
pub mod flash;
pub mod initialize;
pub mod mint;
pub mod pool_created;
pub mod swap;
