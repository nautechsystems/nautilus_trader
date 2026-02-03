/**
 * Shared low-level type definitions for FFI interop.
 */

/** An opaque pointer to a Rust-allocated object. */
export type Pointer = number;

/** A 64-bit unsigned integer returned from FFI (Bun returns as number or bigint). */
export type U64 = number | bigint;
