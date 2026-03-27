//! Python bindings for event types.
//!
//! This module exposes market data and execution events to Python,
//! enabling callbacks and event streaming from Rust to Python.

#[cfg(feature = "python")]
use pyo3::prelude::*;

use crate::data::{MarketDataEvent, QuoteTick, TimeBar as LiveTimeBar, TradeTick};
use crate::execution::{
    ExecutionEvent, OrderAccepted, OrderCancelled, OrderFilled, OrderModified, OrderRejected,
    OrderSubmitted,
};
use crate::providers::{AccountEvent, PositionEvent};

// ============================================================================
// Market Data Events
// ============================================================================

/// Python wrapper for QuoteTick (best bid/offer update).
#[cfg(feature = "python")]
#[pyclass(name = "QuoteTick")]
#[derive(Clone)]
pub struct PyQuoteTick {
    inner: QuoteTick,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyQuoteTick {
    /// Instrument symbol.
    #[getter]
    fn symbol(&self) -> &str {
        &self.inner.symbol
    }

    /// Exchange code.
    #[getter]
    fn exchange(&self) -> &str {
        &self.inner.exchange
    }

    /// Best bid price.
    #[getter]
    fn bid_price(&self) -> f64 {
        self.inner.bid_price
    }

    /// Best ask price.
    #[getter]
    fn ask_price(&self) -> f64 {
        self.inner.ask_price
    }

    /// Bid size.
    #[getter]
    fn bid_size(&self) -> f64 {
        self.inner.bid_size
    }

    /// Ask size.
    #[getter]
    fn ask_size(&self) -> f64 {
        self.inner.ask_size
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    /// Initialization timestamp in nanoseconds.
    #[getter]
    fn ts_init(&self) -> u64 {
        self.inner.ts_init
    }

    fn __repr__(&self) -> String {
        format!(
            "QuoteTick(symbol={}, exchange={}, bid={:.6}, ask={:.6}, bid_size={}, ask_size={})",
            self.inner.symbol,
            self.inner.exchange,
            self.inner.bid_price,
            self.inner.ask_price,
            self.inner.bid_size,
            self.inner.ask_size,
        )
    }
}

impl From<QuoteTick> for PyQuoteTick {
    fn from(tick: QuoteTick) -> Self {
        Self { inner: tick }
    }
}

/// Python wrapper for TradeTick (last trade).
#[cfg(feature = "python")]
#[pyclass(name = "TradeTick")]
#[derive(Clone)]
pub struct PyTradeTick {
    inner: TradeTick,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyTradeTick {
    /// Instrument symbol.
    #[getter]
    fn symbol(&self) -> &str {
        &self.inner.symbol
    }

    /// Exchange code.
    #[getter]
    fn exchange(&self) -> &str {
        &self.inner.exchange
    }

    /// Trade price.
    #[getter]
    fn price(&self) -> f64 {
        self.inner.price
    }

    /// Trade size.
    #[getter]
    fn size(&self) -> f64 {
        self.inner.size
    }

    /// Aggressor side ("BUY" or "SELL").
    #[getter]
    fn aggressor_side(&self) -> &str {
        &self.inner.aggressor_side
    }

    /// Trade ID.
    #[getter]
    fn trade_id(&self) -> &str {
        &self.inner.trade_id
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    /// Initialization timestamp in nanoseconds.
    #[getter]
    fn ts_init(&self) -> u64 {
        self.inner.ts_init
    }

    fn __repr__(&self) -> String {
        format!(
            "TradeTick(symbol={}, exchange={}, price={:.6}, size={}, side={})",
            self.inner.symbol,
            self.inner.exchange,
            self.inner.price,
            self.inner.size,
            self.inner.aggressor_side,
        )
    }
}

impl From<TradeTick> for PyTradeTick {
    fn from(tick: TradeTick) -> Self {
        Self { inner: tick }
    }
}

// ============================================================================
// Execution Events
// ============================================================================

/// Python wrapper for OrderSubmitted event.
#[cfg(feature = "python")]
#[pyclass(name = "OrderSubmitted")]
#[derive(Clone)]
pub struct PyOrderSubmitted {
    inner: OrderSubmitted,
}

#[cfg(feature = "python")]
fn side_text(side: Option<rithmic_rs::OrderSide>) -> Option<String> {
    side.map(|value| value.to_string())
}

#[cfg(feature = "python")]
fn order_type_text(order_type: Option<rithmic_rs::OrderType>) -> Option<String> {
    order_type.map(|value| value.to_string())
}

#[cfg(feature = "python")]
fn time_in_force_text(time_in_force: Option<rithmic_rs::TimeInForce>) -> Option<String> {
    time_in_force.map(|value| value.to_string())
}

#[cfg(feature = "python")]
#[pymethods]
impl PyOrderSubmitted {
    /// Client order ID.
    #[getter]
    fn client_order_id(&self) -> &str {
        &self.inner.client_order_id
    }

    /// Venue order ID (may be None until accepted).
    #[getter]
    fn venue_order_id(&self) -> Option<&str> {
        self.inner.venue_order_id.as_deref()
    }

    /// Account ID.
    #[getter]
    fn account_id(&self) -> &str {
        &self.inner.account_id
    }

    /// Instrument symbol when available.
    #[getter]
    fn symbol(&self) -> Option<&str> {
        self.inner.context.symbol.as_deref()
    }

    /// Exchange code when available.
    #[getter]
    fn exchange(&self) -> Option<&str> {
        self.inner.context.exchange.as_deref()
    }

    /// Order side when available.
    #[getter]
    fn side(&self) -> Option<String> {
        side_text(self.inner.context.side)
    }

    /// Order type when available.
    #[getter]
    fn order_type(&self) -> Option<String> {
        order_type_text(self.inner.context.order_type)
    }

    /// Time in force when available.
    #[getter]
    fn time_in_force(&self) -> Option<String> {
        time_in_force_text(self.inner.context.time_in_force)
    }

    /// Original quantity when available.
    #[getter]
    fn quantity(&self) -> Option<f64> {
        self.inner.context.quantity
    }

    /// Cumulative filled quantity when available.
    #[getter]
    fn filled_qty(&self) -> Option<f64> {
        self.inner.context.filled_qty
    }

    /// Remaining quantity when available.
    #[getter]
    fn leaves_qty(&self) -> Option<f64> {
        self.inner.context.leaves_qty
    }

    /// Order price when available.
    #[getter]
    fn price(&self) -> Option<f64> {
        self.inner.context.price
    }

    /// Stop or trigger price when available.
    #[getter]
    fn trigger_price(&self) -> Option<f64> {
        self.inner.context.trigger_price
    }

    /// Average fill price when available.
    #[getter]
    fn avg_price(&self) -> Option<f64> {
        self.inner.context.avg_price
    }

    /// Parent venue basket ID for bracket child notifications.
    #[getter]
    fn original_basket_id(&self) -> Option<&str> {
        self.inner.context.original_basket_id.as_deref()
    }

    /// Linked venue basket IDs for contingent orders.
    #[getter]
    fn linked_basket_ids(&self) -> Vec<String> {
        self.inner.context.linked_basket_ids.clone()
    }

    /// Venue bracket type when available.
    #[getter]
    fn bracket_type(&self) -> Option<&str> {
        self.inner.context.bracket_type.as_deref()
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderSubmitted(client_order_id={}, venue_order_id={:?})",
            self.inner.client_order_id, self.inner.venue_order_id,
        )
    }
}

impl From<OrderSubmitted> for PyOrderSubmitted {
    fn from(event: OrderSubmitted) -> Self {
        Self { inner: event }
    }
}

/// Python wrapper for OrderAccepted event.
#[cfg(feature = "python")]
#[pyclass(name = "OrderAccepted")]
#[derive(Clone)]
pub struct PyOrderAccepted {
    inner: OrderAccepted,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyOrderAccepted {
    /// Client order ID.
    #[getter]
    fn client_order_id(&self) -> &str {
        &self.inner.client_order_id
    }

    /// Venue order ID.
    #[getter]
    fn venue_order_id(&self) -> &str {
        &self.inner.venue_order_id
    }

    /// Account ID.
    #[getter]
    fn account_id(&self) -> &str {
        &self.inner.account_id
    }

    /// Instrument symbol when available.
    #[getter]
    fn symbol(&self) -> Option<&str> {
        self.inner.context.symbol.as_deref()
    }

    /// Exchange code when available.
    #[getter]
    fn exchange(&self) -> Option<&str> {
        self.inner.context.exchange.as_deref()
    }

    /// Order side when available.
    #[getter]
    fn side(&self) -> Option<String> {
        side_text(self.inner.context.side)
    }

    /// Order type when available.
    #[getter]
    fn order_type(&self) -> Option<String> {
        order_type_text(self.inner.context.order_type)
    }

    /// Time in force when available.
    #[getter]
    fn time_in_force(&self) -> Option<String> {
        time_in_force_text(self.inner.context.time_in_force)
    }

    /// Original quantity when available.
    #[getter]
    fn quantity(&self) -> Option<f64> {
        self.inner.context.quantity
    }

    /// Cumulative filled quantity when available.
    #[getter]
    fn filled_qty(&self) -> Option<f64> {
        self.inner.context.filled_qty
    }

    /// Remaining quantity when available.
    #[getter]
    fn leaves_qty(&self) -> Option<f64> {
        self.inner.context.leaves_qty
    }

    /// Order price when available.
    #[getter]
    fn price(&self) -> Option<f64> {
        self.inner.context.price
    }

    /// Stop or trigger price when available.
    #[getter]
    fn trigger_price(&self) -> Option<f64> {
        self.inner.context.trigger_price
    }

    /// Average fill price when available.
    #[getter]
    fn avg_price(&self) -> Option<f64> {
        self.inner.context.avg_price
    }

    /// Parent venue basket ID for bracket child notifications.
    #[getter]
    fn original_basket_id(&self) -> Option<&str> {
        self.inner.context.original_basket_id.as_deref()
    }

    /// Linked venue basket IDs for contingent orders.
    #[getter]
    fn linked_basket_ids(&self) -> Vec<String> {
        self.inner.context.linked_basket_ids.clone()
    }

    /// Venue bracket type when available.
    #[getter]
    fn bracket_type(&self) -> Option<&str> {
        self.inner.context.bracket_type.as_deref()
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderAccepted(client_order_id={}, venue_order_id={})",
            self.inner.client_order_id, self.inner.venue_order_id,
        )
    }
}

impl From<OrderAccepted> for PyOrderAccepted {
    fn from(event: OrderAccepted) -> Self {
        Self { inner: event }
    }
}

/// Python wrapper for OrderRejected event.
#[cfg(feature = "python")]
#[pyclass(name = "OrderRejected")]
#[derive(Clone)]
pub struct PyOrderRejected {
    inner: OrderRejected,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyOrderRejected {
    /// Client order ID.
    #[getter]
    fn client_order_id(&self) -> &str {
        &self.inner.client_order_id
    }

    /// Rejection reason.
    #[getter]
    fn reason(&self) -> &str {
        &self.inner.reason
    }

    /// Instrument symbol when available.
    #[getter]
    fn symbol(&self) -> Option<&str> {
        self.inner.context.symbol.as_deref()
    }

    /// Exchange code when available.
    #[getter]
    fn exchange(&self) -> Option<&str> {
        self.inner.context.exchange.as_deref()
    }

    /// Parent venue basket ID for bracket child notifications.
    #[getter]
    fn original_basket_id(&self) -> Option<&str> {
        self.inner.context.original_basket_id.as_deref()
    }

    /// Linked venue basket IDs for contingent orders.
    #[getter]
    fn linked_basket_ids(&self) -> Vec<String> {
        self.inner.context.linked_basket_ids.clone()
    }

    /// Venue bracket type when available.
    #[getter]
    fn bracket_type(&self) -> Option<&str> {
        self.inner.context.bracket_type.as_deref()
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderRejected(client_order_id={}, reason={})",
            self.inner.client_order_id, self.inner.reason,
        )
    }
}

impl From<OrderRejected> for PyOrderRejected {
    fn from(event: OrderRejected) -> Self {
        Self { inner: event }
    }
}

/// Python wrapper for OrderFilled event.
#[cfg(feature = "python")]
#[pyclass(name = "OrderFilled")]
#[derive(Clone)]
pub struct PyOrderFilled {
    inner: OrderFilled,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyOrderFilled {
    /// Client order ID.
    #[getter]
    fn client_order_id(&self) -> &str {
        &self.inner.client_order_id
    }

    /// Venue order ID.
    #[getter]
    fn venue_order_id(&self) -> &str {
        &self.inner.venue_order_id
    }

    /// Fill price.
    #[getter]
    fn fill_price(&self) -> f64 {
        self.inner.fill_price
    }

    /// Fill quantity.
    #[getter]
    fn fill_qty(&self) -> f64 {
        self.inner.fill_qty
    }

    /// Remaining quantity.
    #[getter]
    fn leaves_qty(&self) -> f64 {
        self.inner.leaves_qty
    }

    /// Commission.
    #[getter]
    fn commission(&self) -> f64 {
        self.inner.commission
    }

    /// Instrument symbol when available.
    #[getter]
    fn symbol(&self) -> Option<&str> {
        self.inner.context.symbol.as_deref()
    }

    /// Exchange code when available.
    #[getter]
    fn exchange(&self) -> Option<&str> {
        self.inner.context.exchange.as_deref()
    }

    /// Order side when available.
    #[getter]
    fn side(&self) -> Option<String> {
        side_text(self.inner.context.side)
    }

    /// Venue trade identifier when available.
    #[getter]
    fn trade_id(&self) -> Option<&str> {
        self.inner.trade_id.as_deref()
    }

    /// Fill currency when available.
    #[getter]
    fn currency(&self) -> Option<&str> {
        self.inner.currency.as_deref()
    }

    /// Parent venue basket ID for bracket child notifications.
    #[getter]
    fn original_basket_id(&self) -> Option<&str> {
        self.inner.context.original_basket_id.as_deref()
    }

    /// Linked venue basket IDs for contingent orders.
    #[getter]
    fn linked_basket_ids(&self) -> Vec<String> {
        self.inner.context.linked_basket_ids.clone()
    }

    /// Venue bracket type when available.
    #[getter]
    fn bracket_type(&self) -> Option<&str> {
        self.inner.context.bracket_type.as_deref()
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderFilled(client_order_id={}, fill_price={:.6}, fill_qty={}, leaves_qty={})",
            self.inner.client_order_id,
            self.inner.fill_price,
            self.inner.fill_qty,
            self.inner.leaves_qty,
        )
    }
}

impl From<OrderFilled> for PyOrderFilled {
    fn from(event: OrderFilled) -> Self {
        Self { inner: event }
    }
}

/// Python wrapper for OrderCancelled event.
#[cfg(feature = "python")]
#[pyclass(name = "OrderCancelled")]
#[derive(Clone)]
pub struct PyOrderCancelled {
    inner: OrderCancelled,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyOrderCancelled {
    /// Client order ID.
    #[getter]
    fn client_order_id(&self) -> &str {
        &self.inner.client_order_id
    }

    /// Venue order ID.
    #[getter]
    fn venue_order_id(&self) -> &str {
        &self.inner.venue_order_id
    }

    /// Instrument symbol when available.
    #[getter]
    fn symbol(&self) -> Option<&str> {
        self.inner.context.symbol.as_deref()
    }

    /// Exchange code when available.
    #[getter]
    fn exchange(&self) -> Option<&str> {
        self.inner.context.exchange.as_deref()
    }

    /// Parent venue basket ID for bracket child notifications.
    #[getter]
    fn original_basket_id(&self) -> Option<&str> {
        self.inner.context.original_basket_id.as_deref()
    }

    /// Linked venue basket IDs for contingent orders.
    #[getter]
    fn linked_basket_ids(&self) -> Vec<String> {
        self.inner.context.linked_basket_ids.clone()
    }

    /// Venue bracket type when available.
    #[getter]
    fn bracket_type(&self) -> Option<&str> {
        self.inner.context.bracket_type.as_deref()
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderCancelled(client_order_id={}, venue_order_id={})",
            self.inner.client_order_id, self.inner.venue_order_id,
        )
    }
}

impl From<OrderCancelled> for PyOrderCancelled {
    fn from(event: OrderCancelled) -> Self {
        Self { inner: event }
    }
}

/// Python wrapper for OrderModified event.
#[cfg(feature = "python")]
#[pyclass(name = "OrderModified")]
#[derive(Clone)]
pub struct PyOrderModified {
    inner: OrderModified,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyOrderModified {
    /// Client order ID.
    #[getter]
    fn client_order_id(&self) -> &str {
        &self.inner.client_order_id
    }

    /// Venue order ID.
    #[getter]
    fn venue_order_id(&self) -> &str {
        &self.inner.venue_order_id
    }

    /// New price (if modified).
    #[getter]
    fn new_price(&self) -> Option<f64> {
        self.inner.new_price
    }

    /// New quantity (if modified).
    #[getter]
    fn new_qty(&self) -> Option<f64> {
        self.inner.new_qty
    }

    /// Instrument symbol when available.
    #[getter]
    fn symbol(&self) -> Option<&str> {
        self.inner.context.symbol.as_deref()
    }

    /// Exchange code when available.
    #[getter]
    fn exchange(&self) -> Option<&str> {
        self.inner.context.exchange.as_deref()
    }

    /// Parent venue basket ID for bracket child notifications.
    #[getter]
    fn original_basket_id(&self) -> Option<&str> {
        self.inner.context.original_basket_id.as_deref()
    }

    /// Linked venue basket IDs for contingent orders.
    #[getter]
    fn linked_basket_ids(&self) -> Vec<String> {
        self.inner.context.linked_basket_ids.clone()
    }

    /// Venue bracket type when available.
    #[getter]
    fn bracket_type(&self) -> Option<&str> {
        self.inner.context.bracket_type.as_deref()
    }

    /// Event timestamp in nanoseconds.
    #[getter]
    fn ts_event(&self) -> u64 {
        self.inner.ts_event
    }

    fn __repr__(&self) -> String {
        format!(
            "OrderModified(client_order_id={}, new_price={:?}, new_qty={:?})",
            self.inner.client_order_id, self.inner.new_price, self.inner.new_qty,
        )
    }
}

impl From<OrderModified> for PyOrderModified {
    fn from(event: OrderModified) -> Self {
        Self { inner: event }
    }
}

// ============================================================================
// Unified Event Wrapper
// ============================================================================

/// Python wrapper for MarketDataEvent (union type).
#[cfg(feature = "python")]
#[pyclass(name = "MarketDataEvent")]
#[derive(Clone)]
pub struct PyMarketDataEvent {
    inner: MarketDataEvent,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyMarketDataEvent {
    /// Returns true if this is a quote event.
    fn is_quote(&self) -> bool {
        matches!(self.inner, MarketDataEvent::Quote(_))
    }

    /// Returns true if this is a trade event.
    fn is_trade(&self) -> bool {
        matches!(self.inner, MarketDataEvent::Trade(_))
    }

    /// Returns true if this is a live time-bar event.
    fn is_bar(&self) -> bool {
        matches!(self.inner, MarketDataEvent::Bar(_))
    }

    /// Returns true if this is a connection state event.
    fn is_connection_state(&self) -> bool {
        matches!(self.inner, MarketDataEvent::ConnectionState(_))
    }

    /// Returns true if this is an error event.
    fn is_error(&self) -> bool {
        matches!(self.inner, MarketDataEvent::Error(_))
    }

    /// Get the quote tick if this is a quote event.
    fn as_quote(&self) -> Option<PyQuoteTick> {
        match &self.inner {
            MarketDataEvent::Quote(q) => Some(PyQuoteTick::from(q.clone())),
            _ => None,
        }
    }

    /// Get the trade tick if this is a trade event.
    fn as_trade(&self) -> Option<PyTradeTick> {
        match &self.inner {
            MarketDataEvent::Trade(t) => Some(PyTradeTick::from(t.clone())),
            _ => None,
        }
    }

    /// Get the time bar if this is a bar event.
    fn as_bar(&self) -> Option<PyTimeBar> {
        match &self.inner {
            MarketDataEvent::Bar(bar) => Some(PyTimeBar::from(bar.clone())),
            _ => None,
        }
    }

    /// Get the connection state as a string if this is a connection state event.
    fn as_connection_state(&self) -> Option<String> {
        match &self.inner {
            MarketDataEvent::ConnectionState(s) => Some(format!("{:?}", s)),
            _ => None,
        }
    }

    /// Get the error message if this is an error event.
    fn as_error(&self) -> Option<String> {
        match &self.inner {
            MarketDataEvent::Error(e) => Some(e.clone()),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            MarketDataEvent::Quote(q) => {
                format!("MarketDataEvent::Quote({}:{})", q.symbol, q.exchange)
            }
            MarketDataEvent::Trade(t) => {
                format!("MarketDataEvent::Trade({}:{})", t.symbol, t.exchange)
            }
            MarketDataEvent::Bar(bar) => format!(
                "MarketDataEvent::Bar({}:{} {:?}/{})",
                bar.symbol, bar.exchange, bar.bar_type, bar.bar_period
            ),
            MarketDataEvent::ConnectionState(s) => {
                format!("MarketDataEvent::ConnectionState({:?})", s)
            }
            MarketDataEvent::Reconnected => "MarketDataEvent::Reconnected".to_string(),
            MarketDataEvent::Authenticated => "MarketDataEvent::Authenticated".to_string(),
            MarketDataEvent::Error(e) => format!("MarketDataEvent::Error({})", e),
        }
    }
}

impl From<MarketDataEvent> for PyMarketDataEvent {
    fn from(event: MarketDataEvent) -> Self {
        Self { inner: event }
    }
}

/// Python wrapper for ExecutionEvent (union type).
#[cfg(feature = "python")]
#[pyclass(name = "ExecutionEvent")]
#[derive(Clone)]
pub struct PyExecutionEvent {
    inner: ExecutionEvent,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyExecutionEvent {
    /// Returns true if this is a submitted event.
    fn is_submitted(&self) -> bool {
        matches!(self.inner, ExecutionEvent::Submitted(_))
    }

    /// Returns true if this is an accepted event.
    fn is_accepted(&self) -> bool {
        matches!(self.inner, ExecutionEvent::Accepted(_))
    }

    /// Returns true if this is a rejected event.
    fn is_rejected(&self) -> bool {
        matches!(self.inner, ExecutionEvent::Rejected(_))
    }

    /// Returns true if this is a filled event.
    fn is_filled(&self) -> bool {
        matches!(self.inner, ExecutionEvent::Filled(_))
    }

    /// Returns true if this is a cancelled event.
    fn is_cancelled(&self) -> bool {
        matches!(self.inner, ExecutionEvent::Cancelled(_))
    }

    /// Returns true if this is a modified event.
    fn is_modified(&self) -> bool {
        matches!(self.inner, ExecutionEvent::Modified(_))
    }

    /// Returns true if this is a connection state event.
    fn is_connection_state(&self) -> bool {
        matches!(self.inner, ExecutionEvent::ConnectionState(_))
    }

    /// Returns true if this is an error event.
    fn is_error(&self) -> bool {
        matches!(self.inner, ExecutionEvent::Error(_))
    }

    /// Get as submitted event.
    fn as_submitted(&self) -> Option<PyOrderSubmitted> {
        match &self.inner {
            ExecutionEvent::Submitted(e) => Some(PyOrderSubmitted::from(e.clone())),
            _ => None,
        }
    }

    /// Get as accepted event.
    fn as_accepted(&self) -> Option<PyOrderAccepted> {
        match &self.inner {
            ExecutionEvent::Accepted(e) => Some(PyOrderAccepted::from(e.clone())),
            _ => None,
        }
    }

    /// Get as rejected event.
    fn as_rejected(&self) -> Option<PyOrderRejected> {
        match &self.inner {
            ExecutionEvent::Rejected(e) => Some(PyOrderRejected::from(e.clone())),
            _ => None,
        }
    }

    /// Get as filled event.
    fn as_filled(&self) -> Option<PyOrderFilled> {
        match &self.inner {
            ExecutionEvent::Filled(e) => Some(PyOrderFilled::from(e.clone())),
            _ => None,
        }
    }

    /// Get as cancelled event.
    fn as_cancelled(&self) -> Option<PyOrderCancelled> {
        match &self.inner {
            ExecutionEvent::Cancelled(e) => Some(PyOrderCancelled::from(e.clone())),
            _ => None,
        }
    }

    /// Get as modified event.
    fn as_modified(&self) -> Option<PyOrderModified> {
        match &self.inner {
            ExecutionEvent::Modified(e) => Some(PyOrderModified::from(e.clone())),
            _ => None,
        }
    }

    /// Get the connection state as a string if this is a connection state event.
    fn as_connection_state(&self) -> Option<String> {
        match &self.inner {
            ExecutionEvent::ConnectionState(s) => Some(format!("{:?}", s)),
            _ => None,
        }
    }

    /// Get the error message if this is an error event.
    fn as_error(&self) -> Option<String> {
        match &self.inner {
            ExecutionEvent::Error(e) => Some(e.clone()),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            ExecutionEvent::Submitted(e) => {
                format!("ExecutionEvent::Submitted({})", e.client_order_id)
            }
            ExecutionEvent::Accepted(e) => {
                format!("ExecutionEvent::Accepted({})", e.client_order_id)
            }
            ExecutionEvent::Rejected(e) => {
                format!("ExecutionEvent::Rejected({})", e.client_order_id)
            }
            ExecutionEvent::Filled(e) => format!("ExecutionEvent::Filled({})", e.client_order_id),
            ExecutionEvent::Cancelled(e) => {
                format!("ExecutionEvent::Cancelled({})", e.client_order_id)
            }
            ExecutionEvent::Modified(e) => {
                format!("ExecutionEvent::Modified({})", e.client_order_id)
            }
            ExecutionEvent::ConnectionState(s) => {
                format!("ExecutionEvent::ConnectionState({:?})", s)
            }
            ExecutionEvent::Reconnected => "ExecutionEvent::Reconnected".to_string(),
            ExecutionEvent::Authenticated => "ExecutionEvent::Authenticated".to_string(),
            ExecutionEvent::Error(e) => format!("ExecutionEvent::Error({})", e),
        }
    }
}

impl From<ExecutionEvent> for PyExecutionEvent {
    fn from(event: ExecutionEvent) -> Self {
        Self { inner: event }
    }
}

// ============================================================================
// PnL / Position Events
// ============================================================================

/// Python wrapper for AccountEvent.
#[cfg(feature = "python")]
#[pyclass(name = "AccountEvent")]
#[derive(Clone)]
pub struct PyAccountEvent {
    inner: AccountEvent,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyAccountEvent {
    #[getter]
    fn account_id(&self) -> &str {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => &b.account_id,
            AccountEvent::MarginWarning { account_id, .. } => account_id,
            AccountEvent::Error(_) => "",
        }
    }

    #[getter]
    fn currency(&self) -> &str {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => &b.currency,
            AccountEvent::MarginWarning { .. } => "",
            AccountEvent::Error(_) => "",
        }
    }

    #[getter]
    fn total(&self) -> f64 {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => b.total,
            AccountEvent::MarginWarning { .. } => 0.0,
            AccountEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn available(&self) -> f64 {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => b.available,
            AccountEvent::MarginWarning { .. } => 0.0,
            AccountEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn locked(&self) -> f64 {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => b.locked,
            AccountEvent::MarginWarning { .. } => 0.0,
            AccountEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn unrealized_pnl(&self) -> f64 {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => b.unrealized_pnl,
            AccountEvent::MarginWarning { .. } => 0.0,
            AccountEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn realized_pnl(&self) -> f64 {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => b.realized_pnl,
            AccountEvent::MarginWarning { .. } => 0.0,
            AccountEvent::Error(_) => 0.0,
        }
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            AccountEvent::BalanceUpdate(b) => format!(
                "AccountEvent::BalanceUpdate(account={}, total={}, available={}, locked={})",
                b.account_id, b.total, b.available, b.locked
            ),
            AccountEvent::MarginWarning {
                account_id,
                message,
            } => {
                format!("AccountEvent::MarginWarning(account={account_id}, message={message})")
            }
            AccountEvent::Error(err) => format!("AccountEvent::Error({err})"),
        }
    }
}

impl From<AccountEvent> for PyAccountEvent {
    fn from(event: AccountEvent) -> Self {
        Self { inner: event }
    }
}

/// Python wrapper for PositionEvent.
#[cfg(feature = "python")]
#[pyclass(name = "PositionEvent")]
#[derive(Clone)]
pub struct PyPositionEvent {
    inner: PositionEvent,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyPositionEvent {
    #[getter]
    fn account_id(&self) -> &str {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => &p.account_id,
            PositionEvent::Closed { account_id, .. } => account_id,
            PositionEvent::Error(_) => "",
        }
    }

    #[getter]
    fn symbol(&self) -> &str {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => &p.symbol,
            PositionEvent::Closed { symbol, .. } => symbol,
            PositionEvent::Error(_) => "",
        }
    }

    #[getter]
    fn exchange(&self) -> &str {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => &p.exchange,
            PositionEvent::Closed { exchange, .. } => exchange,
            PositionEvent::Error(_) => "",
        }
    }

    #[getter]
    fn quantity(&self) -> f64 {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => p.quantity,
            PositionEvent::Closed { .. } => 0.0,
            PositionEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn avg_price(&self) -> f64 {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => p.avg_price,
            PositionEvent::Closed { .. } => 0.0,
            PositionEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn unrealized_pnl(&self) -> f64 {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => p.unrealized_pnl,
            PositionEvent::Closed { .. } => 0.0,
            PositionEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn realized_pnl(&self) -> f64 {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => p.realized_pnl,
            PositionEvent::Closed { realized_pnl, .. } => *realized_pnl,
            PositionEvent::Error(_) => 0.0,
        }
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => p.ts_event,
            PositionEvent::Closed { .. } => 0,
            PositionEvent::Error(_) => 0,
        }
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            PositionEvent::Updated(p) | PositionEvent::Opened(p) => format!(
                "PositionEvent::Updated(account={}, symbol={}.{}, qty={}, avg_price={})",
                p.account_id, p.symbol, p.exchange, p.quantity, p.avg_price
            ),
            PositionEvent::Closed {
                account_id,
                symbol,
                exchange,
                ..
            } => format!(
                "PositionEvent::Closed(account={}, symbol={}.{})",
                account_id, symbol, exchange
            ),
            PositionEvent::Error(err) => format!("PositionEvent::Error({err})"),
        }
    }
}

impl From<PositionEvent> for PyPositionEvent {
    fn from(event: PositionEvent) -> Self {
        Self { inner: event }
    }
}

// ============================================================================
// Time Bar Data
// ============================================================================

/// Python wrapper for Rithmic time bar data from history requests and live updates.
#[cfg(feature = "python")]
#[pyclass(name = "TimeBar")]
#[derive(Clone)]
pub struct PyTimeBar {
    /// Open price.
    pub open_price: f64,
    /// High price.
    pub high_price: f64,
    /// Low price.
    pub low_price: f64,
    /// Close price.
    pub close_price: f64,
    /// Volume.
    pub volume: i64,
    /// Raw Rithmic period field.
    pub period: String,
    /// Parsed Rithmic bar type name.
    pub bar_kind: String,
    /// Parsed Rithmic bar period/step.
    pub bar_period: i32,
    /// Raw Rithmic bar marker.
    pub marker: Option<i64>,
    /// Event timestamp in nanoseconds.
    pub ts_event: u64,
    /// Initialization timestamp in nanoseconds.
    pub ts_init: u64,
}

#[cfg(feature = "python")]
#[pymethods]
impl PyTimeBar {
    #[getter]
    fn open_price(&self) -> f64 {
        self.open_price
    }

    #[getter]
    fn high_price(&self) -> f64 {
        self.high_price
    }

    #[getter]
    fn low_price(&self) -> f64 {
        self.low_price
    }

    #[getter]
    fn close_price(&self) -> f64 {
        self.close_price
    }

    #[getter]
    fn volume(&self) -> i64 {
        self.volume
    }

    #[getter]
    fn period(&self) -> &str {
        &self.period
    }

    #[getter]
    fn bar_kind(&self) -> &str {
        &self.bar_kind
    }

    #[getter]
    fn bar_period(&self) -> i32 {
        self.bar_period
    }

    #[getter]
    fn marker(&self) -> Option<i64> {
        self.marker
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init
    }

    fn __repr__(&self) -> String {
        format!(
            "TimeBar(type={}, period={}, marker={:?}, o={:.2}, h={:.2}, l={:.2}, c={:.2}, v={})",
            self.bar_kind,
            self.bar_period,
            self.marker,
            self.open_price,
            self.high_price,
            self.low_price,
            self.close_price,
            self.volume
        )
    }
}

impl PyTimeBar {
    /// Creates a PyTimeBar from a Rithmic ResponseTimeBarReplay message.
    pub fn from_response(bar: &rithmic_rs::rti::ResponseTimeBarReplay) -> Self {
        let marker = bar.marker.map(i64::from);
        let ts_event = marker
            .filter(|value| *value > 0)
            .map(|value| value as u64 * 1_000_000_000)
            .or_else(|| {
                bar.period
                    .as_deref()
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|value| value * 1_000_000_000)
            })
            .unwrap_or(0);

        Self {
            open_price: bar.open_price.unwrap_or(0.0),
            high_price: bar.high_price.unwrap_or(0.0),
            low_price: bar.low_price.unwrap_or(0.0),
            close_price: bar.close_price.unwrap_or(0.0),
            volume: bar.volume.unwrap_or(0) as i64,
            period: bar.period.clone().unwrap_or_default(),
            bar_kind: match bar
                .r#type
                .and_then(|value| crate::TimeBarType::try_from(value).ok())
                .unwrap_or(crate::TimeBarType::MinuteBar)
            {
                crate::TimeBarType::SecondBar => "SecondBar".to_string(),
                crate::TimeBarType::MinuteBar => "MinuteBar".to_string(),
                crate::TimeBarType::DailyBar => "DailyBar".to_string(),
                crate::TimeBarType::WeeklyBar => "WeeklyBar".to_string(),
            },
            bar_period: bar
                .period
                .as_deref()
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or_default(),
            marker,
            ts_event,
            ts_init: ts_event,
        }
    }
}

impl From<LiveTimeBar> for PyTimeBar {
    fn from(bar: LiveTimeBar) -> Self {
        Self {
            open_price: bar.open_price,
            high_price: bar.high_price,
            low_price: bar.low_price,
            close_price: bar.close_price,
            volume: bar.volume as i64,
            period: bar.bar_period.to_string(),
            bar_kind: match bar.bar_type {
                crate::TimeBarType::SecondBar => "SecondBar".to_string(),
                crate::TimeBarType::MinuteBar => "MinuteBar".to_string(),
                crate::TimeBarType::DailyBar => "DailyBar".to_string(),
                crate::TimeBarType::WeeklyBar => "WeeklyBar".to_string(),
            },
            bar_period: bar.bar_period,
            marker: bar.marker,
            ts_event: bar.ts_event,
            ts_init: bar.ts_init,
        }
    }
}

/// Registers event types with the Python module.
#[cfg(feature = "python")]
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Market data events
    m.add_class::<PyQuoteTick>()?;
    m.add_class::<PyTradeTick>()?;
    m.add_class::<PyMarketDataEvent>()?;
    m.add_class::<PyTimeBar>()?;

    // Execution events
    m.add_class::<PyOrderSubmitted>()?;
    m.add_class::<PyOrderAccepted>()?;
    m.add_class::<PyOrderRejected>()?;
    m.add_class::<PyOrderFilled>()?;
    m.add_class::<PyOrderCancelled>()?;
    m.add_class::<PyOrderModified>()?;
    m.add_class::<PyExecutionEvent>()?;

    // PnL / Position events
    m.add_class::<PyAccountEvent>()?;
    m.add_class::<PyPositionEvent>()?;

    Ok(())
}
