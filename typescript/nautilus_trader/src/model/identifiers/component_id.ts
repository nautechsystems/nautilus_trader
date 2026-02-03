/**
 * Represents a valid component ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class ComponentId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): ComponentId {
    const raw = getLib().symbols.component_id_new(toCString(value));
    return new ComponentId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.component_id_hash(buf as unknown as Pointer) as bigint;
  }
}
