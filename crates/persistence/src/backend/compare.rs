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

//! Comparator trait for custom ordering.
//!
//! Vendored from the `compare` crate which is unmaintained.
//! Distributed here under MIT (see `licenses/` directory).
//! Original source: <https://github.com/contain-rs/compare>

use std::cmp::Ordering::{self, Equal, Greater, Less};

/// A comparator imposing a total order.
///
/// The `compare` method accepts two values (which may be of the same type or
/// different types) and returns an ordering on them.
///
/// Comparators are useful for parameterizing the behavior of sort methods and
/// certain data structures like binary heaps.
pub trait Compare<L: ?Sized, R: ?Sized = L> {
    /// Compares two values, returning `Less`, `Equal`, or `Greater` if `l` is
    /// less than, equal to, or greater than `r`, respectively.
    fn compare(&self, l: &L, r: &R) -> Ordering;

    /// Checks if `l` is less than `r`.
    fn compares_lt(&self, l: &L, r: &R) -> bool {
        self.compare(l, r) == Less
    }

    /// Checks if `l` is less than or equal to `r`.
    fn compares_le(&self, l: &L, r: &R) -> bool {
        self.compare(l, r) != Greater
    }

    /// Checks if `l` is greater than or equal to `r`.
    fn compares_ge(&self, l: &L, r: &R) -> bool {
        self.compare(l, r) != Less
    }

    /// Checks if `l` is greater than `r`.
    fn compares_gt(&self, l: &L, r: &R) -> bool {
        self.compare(l, r) == Greater
    }

    /// Checks if `l` is equal to `r`.
    fn compares_eq(&self, l: &L, r: &R) -> bool {
        self.compare(l, r) == Equal
    }

    /// Checks if `l` is not equal to `r`.
    fn compares_ne(&self, l: &L, r: &R) -> bool {
        self.compare(l, r) != Equal
    }
}

/// Blanket implementation for closures.
impl<F, L: ?Sized, R: ?Sized> Compare<L, R> for F
where
    F: Fn(&L, &R) -> Ordering,
{
    fn compare(&self, l: &L, r: &R) -> Ordering {
        (*self)(l, r)
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering::{Equal, Greater, Less};

    use rstest::rstest;

    use super::*;

    struct Natural;

    impl Compare<i32> for Natural {
        fn compare(&self, l: &i32, r: &i32) -> Ordering {
            l.cmp(r)
        }
    }

    #[rstest]
    fn test_compare_trait() {
        let cmp = Natural;
        assert_eq!(cmp.compare(&1, &2), Less);
        assert_eq!(cmp.compare(&2, &1), Greater);
        assert_eq!(cmp.compare(&1, &1), Equal);
    }

    #[rstest]
    fn test_closure_comparator() {
        let cmp = |l: &i32, r: &i32| l.cmp(r);
        assert_eq!(cmp.compare(&1, &2), Less);
        assert_eq!(cmp.compare(&2, &1), Greater);
        assert_eq!(cmp.compare(&1, &1), Equal);
    }

    #[rstest]
    fn test_reversed_ordering() {
        let cmp = |l: &i32, r: &i32| l.cmp(r).reverse();
        assert_eq!(cmp.compare(&1, &2), Greater);
        assert_eq!(cmp.compare(&2, &1), Less);
    }
}
