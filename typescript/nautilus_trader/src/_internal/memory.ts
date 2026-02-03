/**
 * Memory management helpers for FFI interop.
 */
import { CString } from "bun:ffi";
import { getLib } from "../lib";

/**
 * Read a C string from a pointer, returning a JavaScript string.
 * The C string memory is freed after reading via `cstr_drop`.
 */
export function readCStr(cstrPtr: number | bigint): string {
  const p = typeof cstrPtr === "bigint" ? Number(cstrPtr) : cstrPtr;
  if (p === 0) return "";
  const cstr = new CString(p);
  const value = cstr.toString();
  // Pass the raw pointer number directly — FFIType.ptr accepts numbers as raw addresses
  getLib().symbols.cstr_drop(p);
  return value;
}

/**
 * Read a C string from a pointer without freeing it.
 * Use when the pointer is borrowed (not owned by us).
 */
export function readBorrowedCStr(cstrPtr: number | bigint): string {
  const p = typeof cstrPtr === "bigint" ? Number(cstrPtr) : cstrPtr;
  if (p === 0) return "";
  return new CString(p).toString();
}

/**
 * Convert a JavaScript string to a null-terminated buffer suitable for FFI.
 */
export function toCString(value: string): Uint8Array {
  return Buffer.from(value + "\0", "utf-8");
}
