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

//! Clock construction for kernel and component clocks.

use std::{
    cell::{OnceCell, RefCell},
    fmt::Debug,
    rc::Rc,
};

use nautilus_common::{
    clock::{Clock, TestClock},
    enums::Environment,
};

/// Clock source for the kernel clock and fresh component clocks.
///
/// The primary clock is created lazily and shared by the kernel and trader timestamps. Component
/// clocks are freshly constructed so callback registration remains isolated per component.
#[derive(Clone)]
pub struct ClockFactory {
    clock: Rc<OnceCell<Rc<RefCell<dyn Clock>>>>,
    create_clock: Rc<dyn Fn() -> Rc<RefCell<dyn Clock>>>,
}

impl ClockFactory {
    /// Create a [`ClockFactory`] from a re-invocable closure.
    #[must_use]
    pub fn new<F>(factory: F) -> Self
    where
        F: Fn() -> Rc<RefCell<dyn Clock>> + 'static,
    {
        Self {
            clock: Rc::new(OnceCell::new()),
            create_clock: Rc::new(factory),
        }
    }

    /// Create a test-default [`ClockFactory`].
    #[must_use]
    pub fn test_default() -> Self {
        Self::for_environment(Environment::Backtest)
    }

    /// Create the default [`ClockFactory`] for an environment.
    #[must_use]
    pub fn for_environment(environment: Environment) -> Self {
        match environment {
            Environment::Backtest => Self::new(|| Rc::new(RefCell::new(TestClock::new()))),
            Environment::Live | Environment::Sandbox => Self::live_default(),
        }
    }

    /// Return the primary clock.
    #[must_use]
    pub fn clock(&self) -> Rc<RefCell<dyn Clock>> {
        self.clock.get_or_init(|| (self.create_clock)()).clone()
    }

    /// Build a fresh component clock instance.
    #[must_use]
    pub fn create_component_clock(&self) -> Rc<RefCell<dyn Clock>> {
        (self.create_clock)()
    }

    #[cfg(feature = "live")]
    fn live_default() -> Self {
        Self::new(|| {
            Rc::new(RefCell::new(
                nautilus_common::live::clock::LiveClock::default(), // nautilus-import-ok
            ))
        })
    }

    #[cfg(not(feature = "live"))]
    fn live_default() -> Self {
        Self::new(|| {
            panic!(
                "Live/Sandbox environment requires the 'live' feature to be enabled. \
                 Build with `--features live` or supply a clock factory."
            )
        })
    }
}

impl Debug for ClockFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ClockFactory))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_clock_memoizes_primary_clock() {
        let calls = Rc::new(Cell::new(0usize));
        let calls_in_factory = calls.clone();
        let factory = ClockFactory::new(move || {
            calls_in_factory.set(calls_in_factory.get() + 1);
            Rc::new(RefCell::new(TestClock::new()))
        });

        let first = factory.clock();
        let second = factory.clock();

        assert_eq!(calls.get(), 1);
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[rstest]
    fn test_create_component_clock_returns_distinct_clocks() {
        let factory = ClockFactory::test_default();

        let first = factory.create_component_clock();
        let second = factory.create_component_clock();

        assert!(!Rc::ptr_eq(&first, &second));
    }

    #[rstest]
    fn test_for_environment_backtest_uses_test_clock() {
        let factory = ClockFactory::for_environment(Environment::Backtest);
        let clock = factory.clock();

        assert!(clock.borrow_mut().as_any_mut().is::<TestClock>());
    }

    #[rstest]
    fn test_default_uses_test_clock() {
        let factory = ClockFactory::test_default();
        let clock = factory.clock();

        assert!(clock.borrow_mut().as_any_mut().is::<TestClock>());
    }
}
