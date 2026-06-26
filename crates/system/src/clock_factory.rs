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

//! Caller-supplied clock construction seam for live/sandbox systems.
//!
//! The kernel builds its kernel clock and every per-component clock through this factory when one
//! is injected (see [`crate::builder::NautilusKernelBuilder::with_clock_factory`]). When no factory
//! is supplied the kernel falls back to `LiveClock::default()`, preserving today's behavior.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::clock::Clock;

/// Cloneable factory closure invoked to construct a fresh [`Clock`] instance.
///
/// Invoked once for the kernel clock and once per registered component, so it is `Fn`
/// (re-invokable) and wrapped in [`Rc`] (the same factory is shared by the kernel and the
/// [`Trader`](crate::trader::Trader)). Each invocation must return a brand-new clock instance to
/// preserve the per-component-instance invariant.
pub type ClockFactory = Rc<dyn Fn() -> Rc<RefCell<dyn Clock>>>;
