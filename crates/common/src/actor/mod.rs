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

#![allow(unsafe_code)]

use std::any::Any;

use ustr::Ustr;

pub mod data_actor;
#[cfg(feature = "indicators")]
pub(crate) mod indicators;
pub mod registry;

#[cfg(test)]
mod tests;

// Re-exports
pub use data_actor::{DataActor, DataActorCore};

pub trait Actor: Any {
    /// The unique identifier for the actor.
    fn id(&self) -> Ustr;
    /// Handles the `msg`.
    fn handle(&mut self, msg: &dyn Any);
    /// Returns a reference to `self` as `Any`, for downcasting support.
    fn as_any(&self) -> &dyn Any;
    /// Returns a mutable reference to `self` as `Any`, for downcasting support.
    ///
    /// Default implementation simply coerces `&mut Self` to `&mut dyn Any`.
    ///
    /// # Note
    ///
    /// This method is not object-safe and thus only available on sized `Self`.
    fn as_any_mut(&mut self) -> &mut dyn Any
    where
        Self: Sized,
    {
        self
    }
}
