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

//! Lifetime-scoped reference newtypes for values held in the platform cache.
//!
//! Each reference type wraps a [`std::cell::Ref`] or [`std::cell::RefMut`] borrow into a cached
//! cell, hiding the smart-pointer leak from public Cache accessor signatures and providing
//! ergonomic trait impls (`Deref`, `PartialEq` against the inner value, `Display`, `Debug`).
//!
//! The borrow drops with the enclosing scope, which makes any attempt to hold it across a cache
//! mutation panic at runtime: a loud failure beats the silent staleness that a stored clone
//! would produce. Use the corresponding `*_owned` accessor on `Cache` when an owned snapshot is
//! needed for a boundary handover.

use std::{
    cell::{Ref, RefMut},
    fmt::{Debug, Display},
    ops::{Deref, DerefMut},
};

use nautilus_model::{accounts::AccountAny, orders::OrderAny, position::Position};

/// Lifetime-scoped read borrow of a cached account.
///
/// Returned by [`crate::cache::Cache::account`]. The borrow drops with the enclosing scope, which
/// makes any attempt to hold it across a cache mutation panic at runtime: a loud failure beats the
/// silent staleness that a stored clone would produce.
///
/// Method calls on the inner [`AccountAny`] resolve via [`Deref`]; comparisons against `&AccountAny`
/// or owned `AccountAny` values are direct (`account_ref == &account`); `Debug` forwards to the
/// inner record. Use [`cloned`](Self::cloned) when an owned snapshot is required (for example,
/// before crossing a boundary that may dispatch events).
pub struct AccountRef<'a>(Ref<'a, AccountAny>);

impl<'a> AccountRef<'a> {
    /// Wraps the given `Ref` borrow.
    #[must_use]
    pub fn new(inner: Ref<'a, AccountAny>) -> Self {
        Self(inner)
    }

    /// Returns an owned snapshot of the borrowed account.
    ///
    /// Mirrors `Option::cloned` and `Iterator::cloned`; the snapshot will not reflect later
    /// mutations of the underlying cell.
    #[must_use]
    pub fn cloned(&self) -> AccountAny {
        (*self.0).clone()
    }
}

impl Deref for AccountRef<'_> {
    type Target = AccountAny;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<AccountAny> for AccountRef<'_> {
    fn as_ref(&self) -> &AccountAny {
        &self.0
    }
}

impl<'a> From<Ref<'a, AccountAny>> for AccountRef<'a> {
    fn from(inner: Ref<'a, AccountAny>) -> Self {
        Self(inner)
    }
}

impl PartialEq<AccountAny> for AccountRef<'_> {
    fn eq(&self, other: &AccountAny) -> bool {
        &**self == other
    }
}

impl PartialEq<&AccountAny> for AccountRef<'_> {
    fn eq(&self, other: &&AccountAny) -> bool {
        &**self == *other
    }
}

impl Debug for AccountRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

/// Lifetime-scoped exclusive write borrow of a cached account.
///
/// Returned by [`crate::cache::Cache::account_mut`]. While the borrow is alive, no other read or
/// write on the same cell is permitted (enforced at runtime by the underlying [`RefMut`]).
/// Drop the borrow before dispatching events or taking any other cache borrow that may re-enter
/// the same account.
pub struct AccountRefMut<'a>(RefMut<'a, AccountAny>);

impl<'a> AccountRefMut<'a> {
    /// Wraps the given `RefMut` borrow.
    #[must_use]
    pub fn new(inner: RefMut<'a, AccountAny>) -> Self {
        Self(inner)
    }

    /// Returns an owned snapshot of the borrowed account.
    #[must_use]
    pub fn cloned(&self) -> AccountAny {
        (*self.0).clone()
    }
}

impl Deref for AccountRefMut<'_> {
    type Target = AccountAny;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AccountRefMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AsRef<AccountAny> for AccountRefMut<'_> {
    fn as_ref(&self) -> &AccountAny {
        &self.0
    }
}

impl AsMut<AccountAny> for AccountRefMut<'_> {
    fn as_mut(&mut self) -> &mut AccountAny {
        &mut self.0
    }
}

impl<'a> From<RefMut<'a, AccountAny>> for AccountRefMut<'a> {
    fn from(inner: RefMut<'a, AccountAny>) -> Self {
        Self(inner)
    }
}

impl PartialEq<AccountAny> for AccountRefMut<'_> {
    fn eq(&self, other: &AccountAny) -> bool {
        &**self == other
    }
}

impl PartialEq<&AccountAny> for AccountRefMut<'_> {
    fn eq(&self, other: &&AccountAny) -> bool {
        &**self == *other
    }
}

impl Debug for AccountRefMut<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

/// Lifetime-scoped read borrow of a cached order.
///
/// Returned by [`crate::cache::Cache::order`]. The borrow drops with the enclosing scope, which
/// makes any attempt to hold it across a cache mutation panic at runtime: a loud failure beats the
/// silent staleness that a stored clone would produce.
///
/// Method calls on the inner [`OrderAny`] resolve via [`Deref`]; comparisons against `&OrderAny`
/// or owned `OrderAny` values are direct (`order_ref == &order`); `Display` and `Debug` forward
/// to the inner record. Use [`cloned`](Self::cloned) when an owned snapshot is required (for
/// example, before crossing a boundary that may dispatch events).
pub struct OrderRef<'a>(Ref<'a, OrderAny>);

impl<'a> OrderRef<'a> {
    /// Wraps the given `Ref` borrow.
    #[must_use]
    pub fn new(inner: Ref<'a, OrderAny>) -> Self {
        Self(inner)
    }

    /// Returns an owned snapshot of the borrowed order.
    ///
    /// Mirrors `Option::cloned` and `Iterator::cloned`; the snapshot will not reflect later
    /// mutations of the underlying cell.
    #[must_use]
    pub fn cloned(&self) -> OrderAny {
        (*self.0).clone()
    }
}

impl Deref for OrderRef<'_> {
    type Target = OrderAny;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<OrderAny> for OrderRef<'_> {
    fn as_ref(&self) -> &OrderAny {
        &self.0
    }
}

impl<'a> From<Ref<'a, OrderAny>> for OrderRef<'a> {
    fn from(inner: Ref<'a, OrderAny>) -> Self {
        Self(inner)
    }
}

impl PartialEq for OrderRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl PartialEq<OrderAny> for OrderRef<'_> {
    fn eq(&self, other: &OrderAny) -> bool {
        &**self == other
    }
}

impl PartialEq<&OrderAny> for OrderRef<'_> {
    fn eq(&self, other: &&OrderAny) -> bool {
        &**self == *other
    }
}

impl Display for OrderRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl Debug for OrderRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

/// Lifetime-scoped exclusive write borrow of a cached order.
///
/// Returned by [`crate::cache::Cache::order_mut`]. While the borrow is alive, no other read or
/// write on the same cell is permitted (enforced at runtime by the underlying [`RefMut`]).
/// Drop the borrow before dispatching events or taking any other cache borrow that may re-enter
/// the same order.
pub struct OrderRefMut<'a>(RefMut<'a, OrderAny>);

impl<'a> OrderRefMut<'a> {
    /// Wraps the given `RefMut` borrow.
    #[must_use]
    pub fn new(inner: RefMut<'a, OrderAny>) -> Self {
        Self(inner)
    }

    /// Returns an owned snapshot of the borrowed order.
    #[must_use]
    pub fn cloned(&self) -> OrderAny {
        (*self.0).clone()
    }
}

impl Deref for OrderRefMut<'_> {
    type Target = OrderAny;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OrderRefMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AsRef<OrderAny> for OrderRefMut<'_> {
    fn as_ref(&self) -> &OrderAny {
        &self.0
    }
}

impl AsMut<OrderAny> for OrderRefMut<'_> {
    fn as_mut(&mut self) -> &mut OrderAny {
        &mut self.0
    }
}

impl<'a> From<RefMut<'a, OrderAny>> for OrderRefMut<'a> {
    fn from(inner: RefMut<'a, OrderAny>) -> Self {
        Self(inner)
    }
}

impl PartialEq<OrderAny> for OrderRefMut<'_> {
    fn eq(&self, other: &OrderAny) -> bool {
        &**self == other
    }
}

impl PartialEq<&OrderAny> for OrderRefMut<'_> {
    fn eq(&self, other: &&OrderAny) -> bool {
        &**self == *other
    }
}

impl Display for OrderRefMut<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl Debug for OrderRefMut<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

/// Lifetime-scoped read borrow of a cached position.
///
/// Returned by [`crate::cache::Cache::position`]. The borrow drops with the enclosing scope, which
/// makes any attempt to hold it across a cache mutation panic at runtime: a loud failure beats the
/// silent staleness that a stored clone would produce.
///
/// Method calls on the inner [`Position`] resolve via [`Deref`]; comparisons against `&Position`
/// or owned `Position` values are direct (`position_ref == &position`); `Display` and `Debug`
/// forward to the inner record. Use [`cloned`](Self::cloned) when an owned snapshot is required
/// (for example, before crossing a boundary that may dispatch events).
pub struct PositionRef<'a>(Ref<'a, Position>);

impl<'a> PositionRef<'a> {
    /// Wraps the given `Ref` borrow.
    #[must_use]
    pub fn new(inner: Ref<'a, Position>) -> Self {
        Self(inner)
    }

    /// Returns an owned snapshot of the borrowed position.
    ///
    /// Mirrors `Option::cloned` and `Iterator::cloned`; the snapshot will not reflect later
    /// mutations of the underlying cell.
    #[must_use]
    pub fn cloned(&self) -> Position {
        (*self.0).clone()
    }
}

impl Deref for PositionRef<'_> {
    type Target = Position;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Position> for PositionRef<'_> {
    fn as_ref(&self) -> &Position {
        &self.0
    }
}

impl<'a> From<Ref<'a, Position>> for PositionRef<'a> {
    fn from(inner: Ref<'a, Position>) -> Self {
        Self(inner)
    }
}

impl PartialEq for PositionRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl PartialEq<Position> for PositionRef<'_> {
    fn eq(&self, other: &Position) -> bool {
        &**self == other
    }
}

impl PartialEq<&Position> for PositionRef<'_> {
    fn eq(&self, other: &&Position) -> bool {
        &**self == *other
    }
}

impl Display for PositionRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl Debug for PositionRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

/// Lifetime-scoped exclusive write borrow of a cached position.
///
/// Returned by [`crate::cache::Cache::position_mut`]. While the borrow is alive, no other read or
/// write on the same cell is permitted (enforced at runtime by the underlying [`RefMut`]).
/// Drop the borrow before dispatching events or taking any other cache borrow that may re-enter
/// the same position.
pub struct PositionRefMut<'a>(RefMut<'a, Position>);

impl<'a> PositionRefMut<'a> {
    /// Wraps the given `RefMut` borrow.
    #[must_use]
    pub fn new(inner: RefMut<'a, Position>) -> Self {
        Self(inner)
    }

    /// Returns an owned snapshot of the borrowed position.
    #[must_use]
    pub fn cloned(&self) -> Position {
        (*self.0).clone()
    }
}

impl Deref for PositionRefMut<'_> {
    type Target = Position;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PositionRefMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AsRef<Position> for PositionRefMut<'_> {
    fn as_ref(&self) -> &Position {
        &self.0
    }
}

impl AsMut<Position> for PositionRefMut<'_> {
    fn as_mut(&mut self) -> &mut Position {
        &mut self.0
    }
}

impl<'a> From<RefMut<'a, Position>> for PositionRefMut<'a> {
    fn from(inner: RefMut<'a, Position>) -> Self {
        Self(inner)
    }
}

impl PartialEq<Position> for PositionRefMut<'_> {
    fn eq(&self, other: &Position) -> bool {
        &**self == other
    }
}

impl PartialEq<&Position> for PositionRefMut<'_> {
    fn eq(&self, other: &&Position) -> bool {
        &**self == *other
    }
}

impl Display for PositionRefMut<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl Debug for PositionRefMut<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_model::{
        accounts::{Account, CashAccount, stubs::cash_account},
        enums::{AccountType, OrderSide, OrderType},
        events::{AccountState, account::stubs::cash_account_state},
        instruments::{CurrencyPair, stubs::*},
        orders::{Order, builder::OrderTestBuilder},
        stubs::{stub_position_long, stub_position_short},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn make_order(audusd_sim: &CurrencyPair) -> OrderAny {
        OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(audusd_sim.id)
            .side(OrderSide::Buy)
            .price(Price::from("1.00000"))
            .quantity(Quantity::from(100_000))
            .build()
    }

    fn make_account(state: AccountState) -> AccountAny {
        AccountAny::Cash(CashAccount::new(state, true, false))
    }

    #[rstest]
    fn test_account_ref_partial_eq_against_owned(cash_account_state: AccountState) {
        let account = make_account(cash_account_state);
        let cell = Rc::new(RefCell::new(account.clone()));
        let account_ref = AccountRef::from(cell.borrow());

        assert_eq!(account_ref, account);
        assert_eq!(account_ref, &account);
    }

    #[rstest]
    fn test_account_ref_debug_matches_inner(cash_account_state: AccountState) {
        let account = make_account(cash_account_state);
        let cell = Rc::new(RefCell::new(account.clone()));
        let account_ref = AccountRef::from(cell.borrow());

        assert_eq!(format!("{account_ref:?}"), format!("{account:?}"));
    }

    #[rstest]
    fn test_account_ref_cloned_is_independent(cash_account_state: AccountState) {
        let cell = Rc::new(RefCell::new(make_account(cash_account_state)));
        let snapshot = AccountRef::from(cell.borrow()).cloned();

        assert_eq!(snapshot, *cell.borrow());
    }

    #[rstest]
    fn test_account_ref_deref_method_call(cash_account: CashAccount) {
        let account = AccountAny::Cash(cash_account);
        let cell = Rc::new(RefCell::new(account.clone()));
        let account_ref = AccountRef::from(cell.borrow());

        // Methods on `AccountAny` are reachable via `Deref`.
        assert_eq!(account_ref.id(), account.id());
        assert_eq!(account_ref.account_type(), AccountType::Cash);
    }

    #[rstest]
    fn test_account_ref_mut_writes_through_deref_mut(cash_account_state: AccountState) {
        let cell = Rc::new(RefCell::new(make_account(cash_account_state.clone())));
        let mut account_mut = AccountRefMut::from(cell.borrow_mut());

        // Apply the same state event again; the apply succeeds and mutates events.
        account_mut.apply(cash_account_state).unwrap();
        let event_count = account_mut.events().len();
        drop(account_mut);

        assert_eq!(cell.borrow().events().len(), event_count);
    }

    #[rstest]
    fn test_account_ref_mut_partial_eq_against_owned(cash_account_state: AccountState) {
        let account = make_account(cash_account_state);
        let cell = Rc::new(RefCell::new(account.clone()));
        let account_mut = AccountRefMut::from(cell.borrow_mut());

        assert_eq!(account_mut, account);
        assert_eq!(account_mut, &account);
    }

    #[rstest]
    fn test_order_ref_partial_eq_against_owned(audusd_sim: CurrencyPair) {
        let order = make_order(&audusd_sim);
        let cell = Rc::new(RefCell::new(order.clone()));
        let order_ref = OrderRef::from(cell.borrow());

        assert_eq!(order_ref, order);
        assert_eq!(order_ref, &order);
    }

    #[rstest]
    fn test_order_ref_display_matches_inner(audusd_sim: CurrencyPair) {
        let order = make_order(&audusd_sim);
        let cell = Rc::new(RefCell::new(order.clone()));
        let order_ref = OrderRef::from(cell.borrow());

        assert_eq!(format!("{order_ref}"), format!("{order}"));
        assert_eq!(format!("{order_ref:?}"), format!("{order:?}"));
    }

    #[rstest]
    fn test_order_ref_cloned_is_independent(audusd_sim: CurrencyPair) {
        let cell = Rc::new(RefCell::new(make_order(&audusd_sim)));
        let snapshot = OrderRef::from(cell.borrow()).cloned();
        let original_qty = snapshot.quantity();

        // Snapshot is independent: subsequent mutation of the cell does not affect it.
        cell.borrow_mut().set_quantity(Quantity::from(1));

        assert_eq!(snapshot.quantity(), original_qty);
        assert_eq!(cell.borrow().quantity(), Quantity::from(1));
    }

    #[rstest]
    fn test_order_ref_deref_method_call(audusd_sim: CurrencyPair) {
        let order = make_order(&audusd_sim);
        let cell = Rc::new(RefCell::new(order.clone()));
        let order_ref = OrderRef::from(cell.borrow());

        // Methods on `OrderAny` are reachable via `Deref`.
        assert_eq!(order_ref.client_order_id(), order.client_order_id());
        assert_eq!(order_ref.quantity(), order.quantity());
    }

    #[rstest]
    fn test_order_ref_mut_writes_through_deref_mut(audusd_sim: CurrencyPair) {
        let cell = Rc::new(RefCell::new(make_order(&audusd_sim)));
        let mut order_mut = OrderRefMut::from(cell.borrow_mut());

        order_mut.set_quantity(Quantity::from(7));
        drop(order_mut);

        assert_eq!(cell.borrow().quantity(), Quantity::from(7));
    }

    #[rstest]
    fn test_order_ref_mut_partial_eq_against_owned(audusd_sim: CurrencyPair) {
        let order = make_order(&audusd_sim);
        let cell = Rc::new(RefCell::new(order.clone()));
        let order_mut = OrderRefMut::from(cell.borrow_mut());

        assert_eq!(order_mut, order);
        assert_eq!(order_mut, &order);
    }

    #[rstest]
    fn test_position_ref_partial_eq_against_owned(stub_position_long: Position) {
        let cell = Rc::new(RefCell::new(stub_position_long.clone()));
        let position_ref = PositionRef::from(cell.borrow());

        assert_eq!(position_ref, stub_position_long);
        assert_eq!(position_ref, &stub_position_long);
    }

    #[rstest]
    fn test_position_ref_display_matches_inner(stub_position_long: Position) {
        let cell = Rc::new(RefCell::new(stub_position_long.clone()));
        let position_ref = PositionRef::from(cell.borrow());

        assert_eq!(format!("{position_ref}"), format!("{stub_position_long}"));
        assert_eq!(
            format!("{position_ref:?}"),
            format!("{stub_position_long:?}")
        );
    }

    #[rstest]
    fn test_position_ref_cloned_is_independent(stub_position_short: Position) {
        let cell = Rc::new(RefCell::new(stub_position_short));
        let snapshot = PositionRef::from(cell.borrow()).cloned();
        let original_qty = snapshot.quantity;

        cell.borrow_mut().quantity = Quantity::from(1);

        assert_eq!(snapshot.quantity, original_qty);
        assert_eq!(cell.borrow().quantity, Quantity::from(1));
    }

    #[rstest]
    fn test_position_ref_deref_method_call(stub_position_long: Position) {
        let cell = Rc::new(RefCell::new(stub_position_long.clone()));
        let position_ref = PositionRef::from(cell.borrow());

        // Fields and methods on `Position` are reachable via `Deref`.
        assert_eq!(position_ref.id, stub_position_long.id);
        assert_eq!(position_ref.quantity, stub_position_long.quantity);
    }

    #[rstest]
    fn test_position_ref_mut_writes_through_deref_mut(stub_position_long: Position) {
        let cell = Rc::new(RefCell::new(stub_position_long));
        let mut position_mut = PositionRefMut::from(cell.borrow_mut());

        position_mut.quantity = Quantity::from(7);
        drop(position_mut);

        assert_eq!(cell.borrow().quantity, Quantity::from(7));
    }

    #[rstest]
    fn test_position_ref_mut_partial_eq_against_owned(stub_position_long: Position) {
        let cell = Rc::new(RefCell::new(stub_position_long.clone()));
        let position_mut = PositionRefMut::from(cell.borrow_mut());

        assert_eq!(position_mut, stub_position_long);
        assert_eq!(position_mut, &stub_position_long);
    }
}
