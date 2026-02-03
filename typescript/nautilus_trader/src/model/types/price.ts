/**
 * Represents a price with a fixed-point decimal precision.
 *
 * The underlying Rust struct (16 bytes) is heap-allocated via `bun_price_*`
 * wrapper functions. Call `close()` to free the memory.
 */
import { getLib } from "../../lib";
import { readCStr } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

const registry = new FinalizationRegistry((ptr: Pointer) => {
  if (ptr !== 0) {
    getLib().symbols.bun_price_drop(ptr);
  }
});

export class Price {
  private _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
    registry.register(this, ptr);
  }

  /** Create a Price from a float value with the given decimal precision. */
  static fromFloat(value: number, precision: number): Price {
    const ptr = getLib().symbols.bun_price_new(value, precision) as number;
    return new Price(ptr);
  }

  /** Get the price as a float64 value. */
  asFloat(): number {
    return getLib().symbols.bun_price_as_f64(this._ptr) as number;
  }

  /** Get the decimal precision. */
  get precision(): number {
    return getLib().symbols.bun_price_precision(this._ptr) as number;
  }

  /** Return the formatted string representation. */
  toString(): string {
    const cstrPtr = getLib().symbols.bun_price_to_cstr(this._ptr);
    return readCStr(cstrPtr as number);
  }

  /** Free the underlying Rust allocation. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_price_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
