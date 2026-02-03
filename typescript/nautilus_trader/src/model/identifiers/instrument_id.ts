/**
 * Represents a valid instrument ID (symbol + venue).
 *
 * The underlying Rust struct (16 bytes) is heap-allocated via `bun_instrument_id_*`
 * wrapper functions. Call `close()` to free the memory.
 */
import { getLib } from "../../lib";
import { readCStr, toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

const registry = new FinalizationRegistry((ptr: Pointer) => {
  if (ptr !== 0) {
    getLib().symbols.bun_instrument_id_drop(ptr);
  }
});

export class InstrumentId {
  private _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
    registry.register(this, ptr);
  }

  /** Create an InstrumentId from a string like "BTCUSDT.BINANCE". */
  static from(value: string): InstrumentId {
    const ptr = getLib().symbols.bun_instrument_id_from_cstr(
      toCString(value),
    ) as number;
    return new InstrumentId(ptr);
  }

  /** Convert to string representation. */
  toString(): string {
    const cstrPtr = getLib().symbols.bun_instrument_id_to_cstr(this._ptr);
    return readCStr(cstrPtr as number);
  }

  /** Compute the stable u64 hash. */
  hash(): bigint {
    return getLib().symbols.bun_instrument_id_hash(this._ptr) as bigint;
  }

  /** Check if this instrument ID is synthetic. */
  isSynthetic(): boolean {
    return getLib().symbols.bun_instrument_id_is_synthetic(this._ptr) === 1;
  }

  /** Free the underlying Rust allocation. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_instrument_id_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
