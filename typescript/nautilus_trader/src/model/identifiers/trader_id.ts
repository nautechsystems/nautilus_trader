/**
 * Represents a valid trader ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class TraderId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): TraderId {
    const raw = getLib().symbols.trader_id_new(toCString(value));
    return new TraderId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.trader_id_hash(buf as unknown as Pointer) as bigint;
  }
}
