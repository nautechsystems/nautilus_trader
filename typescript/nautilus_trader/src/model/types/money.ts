/**
 * Represents a monetary amount with an associated currency.
 *
 * The underlying Rust struct (40 bytes) is heap-allocated via `bun_money_*`
 * wrapper functions. Call `close()` to free the memory.
 */
import { getLib } from "../../lib";
import { readCStr } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";
import { Currency } from "./currency";

const registry = new FinalizationRegistry((ptr: Pointer) => {
  if (ptr !== 0) {
    getLib().symbols.bun_money_drop(ptr);
  }
});

export class Money {
  private _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
    registry.register(this, ptr);
  }

  /** Create a Money value from a float amount and a Currency. */
  static create(amount: number, currency: Currency): Money {
    const ptr = getLib().symbols.bun_money_new(amount, currency._ptr) as number;
    return new Money(ptr);
  }

  /** Get the amount as a float64 value. */
  asFloat(): number {
    return getLib().symbols.bun_money_as_f64(this._ptr) as number;
  }

  /** Return the formatted string representation (e.g., "100.50 USD"). */
  toString(): string {
    const cstrPtr = getLib().symbols.bun_money_to_cstr(this._ptr);
    return readCStr(cstrPtr as number);
  }

  /** Free the underlying Rust allocation. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_money_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
