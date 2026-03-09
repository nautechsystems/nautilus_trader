//! Explicit, manually-invocable cleanup hook used to break reference cycles before `Drop`.
//!
//! Many long-lived components register callbacks or handlers that retain strong references back to
//! them, creating reference-count cycles that prevent Rust’s automatic destructor (`Drop`) from
//! running.  The `CleanDrop` trait provides an *object-safe* method, `clean_drop`, that can be
//! called explicitly (e.g. during an orderly shutdown) to release such resources.  Implementations
//! should also call `clean_drop` from their `Drop` impl as a final safety net.
//!
//! Design contract:
//! 1. **Idempotent** – multiple calls must be safe.
//! 2. Perform all externally-observable cleanup here (unregister handlers, abort tasks, clear
//!    callbacks, downgrade `Rc`/`Arc` references, etc.).

/// Trait providing an explicit cleanup method that may be invoked prior to `Drop`.
pub trait CleanDrop {
    /// Perform custom cleanup, releasing external resources and breaking strong reference cycles.
    fn clean_drop(&mut self);
}
