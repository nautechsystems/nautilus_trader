/**
 * Represents a valid trade ID.
 *
 * TradeId uses a StackStr internally (38 bytes), so it is heap-allocated
 * via `bun_trade_id_*` wrapper functions. Call `close()` to free.
 */
import { getLib } from "../../lib";
import { readCStr, toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

const registry = new FinalizationRegistry((ptr: Pointer) => {
  if (ptr !== 0) {
    getLib().symbols.bun_trade_id_drop(ptr);
  }
});

export class TradeId {
  private _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
    registry.register(this, ptr);
  }

  static from(value: string): TradeId {
    const ptr = getLib().symbols.bun_trade_id_new(toCString(value)) as number;
    return new TradeId(ptr);
  }

  /** Return the string representation of this TradeId. */
  toString(): string {
    const cstrPtr = getLib().symbols.bun_trade_id_to_cstr(this._ptr);
    return readCStr(cstrPtr as number);
  }

  /** Compute the stable u64 hash. */
  hash(): bigint {
    return getLib().symbols.bun_trade_id_hash(this._ptr) as bigint;
  }

  /** Free the underlying Rust allocation. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_trade_id_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
