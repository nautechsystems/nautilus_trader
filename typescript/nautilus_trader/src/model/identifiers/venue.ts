/**
 * Represents a valid trading venue ID.
 *
 * Venue wraps a Ustr (8 bytes), which is pointer-sized and returned
 * by value from FFI without issue.
 */
import { getLib } from "../../lib";
import { readCStr, toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class Venue {
  /** The raw Ustr pointer value (8 bytes, returned by value from FFI). */
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  /** Create a Venue from a string value. */
  static from(value: string): Venue {
    const raw = getLib().symbols.venue_new(toCString(value));
    return new Venue(raw as number);
  }

  /** Compute the stable u64 hash of this venue. */
  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.venue_hash(buf as unknown as Pointer) as bigint;
  }

  /** Check if this is a synthetic venue. */
  isSynthetic(): boolean {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.venue_is_synthetic(buf as unknown as Pointer) === 1;
  }

  /** Return the string value of this venue. */
  toString(): string {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    const cstrPtr = getLib().symbols.bun_venue_to_cstr(buf as unknown as Pointer);
    return readCStr(cstrPtr as number);
  }
}
