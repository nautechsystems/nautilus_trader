/**
 * Represents a valid client ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class ClientId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): ClientId {
    const raw = getLib().symbols.client_id_new(toCString(value));
    return new ClientId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.client_id_hash(buf as unknown as Pointer) as bigint;
  }
}
