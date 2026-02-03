/**
 * Represents a valid execution algorithm ID.
 */
import { getLib } from "../../lib";
import { toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class ExecAlgorithmId {
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  static from(value: string): ExecAlgorithmId {
    const raw = getLib().symbols.exec_algorithm_id_new(toCString(value));
    return new ExecAlgorithmId(raw as number);
  }

  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.exec_algorithm_id_hash(buf as unknown as Pointer) as bigint;
  }
}
