/**
 * Represents a quantity with a fixed-point decimal precision.
 *
 * The underlying Rust struct (16 bytes) is heap-allocated via `bun_quantity_*`
 * wrapper functions. Call `close()` to free the memory.
 */
import { getLib } from "../../lib";
import { readCStr } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

const registry = new FinalizationRegistry((ptr: Pointer) => {
  if (ptr !== 0) {
    getLib().symbols.bun_quantity_drop(ptr);
  }
});

export class Quantity {
  private _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
    registry.register(this, ptr);
  }

  /** Create a Quantity from a float value with the given decimal precision. */
  static fromFloat(value: number, precision: number): Quantity {
    const ptr = getLib().symbols.bun_quantity_new(value, precision) as number;
    return new Quantity(ptr);
  }

  /** Get the quantity as a float64 value. */
  asFloat(): number {
    return getLib().symbols.bun_quantity_as_f64(this._ptr) as number;
  }

  /** Get the decimal precision. */
  get precision(): number {
    return getLib().symbols.bun_quantity_precision(this._ptr) as number;
  }

  /** Return the formatted string representation. */
  toString(): string {
    const cstrPtr = getLib().symbols.bun_quantity_to_cstr(this._ptr);
    return readCStr(cstrPtr as number);
  }

  /** Free the underlying Rust allocation. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_quantity_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
