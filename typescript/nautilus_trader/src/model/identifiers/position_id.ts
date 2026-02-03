/**
 * Represents a valid position ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class PositionId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): PositionId {
    const raw = getLib().symbols.position_id_new(toCString(value));
    return new PositionId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.position_id_hash(buf as unknown as Pointer) as bigint;
  }
}
