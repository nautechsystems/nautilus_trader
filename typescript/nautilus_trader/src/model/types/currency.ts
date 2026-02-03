/**
 * Represents a currency type with code, precision, and metadata.
 *
 * The underlying Rust struct (32 bytes) is heap-allocated via `bun_currency_*`
 * wrapper functions. Call `close()` to free the memory.
 */
import { getLib } from "../../lib";
import { readCStr, toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

const registry = new FinalizationRegistry((ptr: Pointer) => {
  if (ptr !== 0) {
    getLib().symbols.bun_currency_drop(ptr);
  }
});

export class Currency {
  /** Opaque pointer to the heap-allocated Rust Currency struct. */
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
    registry.register(this, ptr);
  }

  /** Get a currency by its code string (e.g., "USD", "BTC"). */
  static from(code: string): Currency {
    const ptr = getLib().symbols.bun_currency_from_cstr(
      toCString(code),
    ) as number;
    return new Currency(ptr);
  }

  /** Check if a currency code exists in the global registry. */
  static exists(code: string): boolean {
    return getLib().symbols.bun_currency_exists(toCString(code)) === 1;
  }

  /** Get the currency code as a string. */
  code(): string {
    const cstrPtr = getLib().symbols.bun_currency_code_to_cstr(this._ptr);
    return readCStr(cstrPtr as number);
  }

  /** Get the currency name as a string. */
  name(): string {
    const cstrPtr = getLib().symbols.bun_currency_name_to_cstr(this._ptr);
    return readCStr(cstrPtr as number);
  }

  /** Get the decimal precision. */
  get precision(): number {
    return getLib().symbols.bun_currency_precision(this._ptr) as number;
  }

  /** Compute the hash of the currency code. */
  hash(): bigint {
    return getLib().symbols.bun_currency_hash(this._ptr) as bigint;
  }

  toString(): string {
    return this.code();
  }

  /** Free the underlying Rust allocation. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_currency_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
