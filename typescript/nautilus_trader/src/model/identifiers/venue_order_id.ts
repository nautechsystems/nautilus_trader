/**
 * Represents a valid venue order ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class VenueOrderId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): VenueOrderId {
    const raw = getLib().symbols.venue_order_id_new(toCString(value));
    return new VenueOrderId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.venue_order_id_hash(buf as unknown as Pointer) as bigint;
  }
}
