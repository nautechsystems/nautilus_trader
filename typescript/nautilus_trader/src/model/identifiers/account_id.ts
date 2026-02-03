/**
 * Represents a valid account ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class AccountId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): AccountId {
    const raw = getLib().symbols.account_id_new(toCString(value));
    return new AccountId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.account_id_hash(buf as unknown as Pointer) as bigint;
  }
}
