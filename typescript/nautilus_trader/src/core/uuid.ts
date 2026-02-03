/**
 * UUID4 wrapper class that delegates to the Rust FFI implementation.
 *
 * The underlying Rust struct (37 bytes) is heap-allocated via `bun_uuid4_*`
 * wrapper functions. Call `close()` to free the memory, or rely on the
 * `FinalizationRegistry` safety net.
 */
import { getLib } from "../lib";
import { readBorrowedCStr, toCString } from "../_internal/memory";
import type { Pointer } from "../_internal/types";

const registry = new FinalizationRegistry((ptr: Pointer) => {
  if (ptr !== 0) {
    getLib().symbols.bun_uuid4_drop(ptr);
  }
});

/**
 * A universally unique identifier (UUID) version 4 based on a 128-bit value.
 */
export class UUID4 {
  private _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
    registry.register(this, ptr);
  }

  /** Generate a new random UUID4. */
  static create(): UUID4 {
    const ptr = getLib().symbols.bun_uuid4_new() as number;
    return new UUID4(ptr);
  }

  /** Create a UUID4 from a string representation. */
  static fromString(value: string): UUID4 {
    const ptr = getLib().symbols.bun_uuid4_from_cstr(toCString(value)) as number;
    return new UUID4(ptr);
  }

  /** Return the string representation of this UUID4. */
  toString(): string {
    const cstrPtr = getLib().symbols.bun_uuid4_to_cstr(this._ptr);
    // bun_uuid4_to_cstr returns a borrowed pointer — do not free
    return readBorrowedCStr(cstrPtr as number);
  }

  /** Check equality with another UUID4. */
  equals(other: UUID4): boolean {
    return getLib().symbols.bun_uuid4_eq(this._ptr, other._ptr) === 1;
  }

  /** Compute a stable u64 hash of this UUID. */
  hash(): bigint {
    return getLib().symbols.bun_uuid4_hash(this._ptr) as bigint;
  }

  /** Free the underlying Rust allocation. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_uuid4_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
