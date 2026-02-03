/**
 * Represents a valid order list ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class OrderListId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): OrderListId {
    const raw = getLib().symbols.order_list_id_new(toCString(value));
    return new OrderListId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.order_list_id_hash(buf as unknown as Pointer) as bigint;
  }
}
