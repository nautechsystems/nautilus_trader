/**
 * Represents a valid ticker symbol ID for a tradable instrument.
 *
 * Symbol wraps a Ustr (8 bytes), which is pointer-sized and returned
 * by value from FFI without issue.
 */
import { getLib } from "../../lib";
import { readCStr, toCString } from "../../_internal/memory";
import type { Pointer } from "../../_internal/types";

export class Symbol {
  /** The raw Ustr pointer value (8 bytes, returned by value from FFI). */
  readonly _ptr: Pointer;

  private constructor(ptr: Pointer) {
    this._ptr = ptr;
  }

  /** Create a Symbol from a string value. */
  static from(value: string): Symbol {
    const raw = getLib().symbols.symbol_new(toCString(value));
    return new Symbol(raw as number);
  }

  /** Compute the stable u64 hash of this symbol. */
  hash(): bigint {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.symbol_hash(buf as unknown as Pointer) as bigint;
  }

  /** Check if this is a composite symbol (contains a period). */
  isComposite(): boolean {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    return getLib().symbols.symbol_is_composite(buf as unknown as Pointer) === 1;
  }

  /** Get the root of this symbol (before the first period). */
  root(): string {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    const cstrPtr = getLib().symbols.symbol_root(buf as unknown as Pointer);
    return readCStr(cstrPtr as number);
  }

  /** Get the topic string for this symbol. */
  topic(): string {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    const cstrPtr = getLib().symbols.symbol_topic(buf as unknown as Pointer);
    return readCStr(cstrPtr as number);
  }

  /** Return the full string value of this symbol. */
  toString(): string {
    const buf = new BigInt64Array([BigInt(this._ptr)]);
    const cstrPtr = getLib().symbols.bun_symbol_to_cstr(buf as unknown as Pointer);
    return readCStr(cstrPtr as number);
  }
}
