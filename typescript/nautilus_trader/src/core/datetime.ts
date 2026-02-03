/**
 * Date/time conversion utilities delegating to the Rust FFI implementation.
 */
import { getLib } from "../lib";
import { readCStr } from "../_internal/memory";

/** Converts seconds to nanoseconds. */
export function secsToNanos(secs: number): bigint {
  return getLib().symbols.secs_to_nanos(secs) as bigint;
}

/** Converts seconds to milliseconds. */
export function secsToMillis(secs: number): bigint {
  return getLib().symbols.secs_to_millis(secs) as bigint;
}

/** Converts milliseconds to nanoseconds. */
export function millisToNanos(millis: number): bigint {
  return getLib().symbols.millis_to_nanos(millis) as bigint;
}

/** Converts microseconds to nanoseconds. */
export function microsToNanos(micros: number): bigint {
  return getLib().symbols.micros_to_nanos(micros) as bigint;
}

/** Converts nanoseconds to seconds. */
export function nanosToSecs(nanos: bigint): number {
  return getLib().symbols.nanos_to_secs(nanos) as number;
}

/** Converts nanoseconds to milliseconds. */
export function nanosToMillis(nanos: bigint): bigint {
  return getLib().symbols.nanos_to_millis(nanos) as bigint;
}

/** Converts nanoseconds to microseconds. */
export function nanosToMicros(nanos: bigint): bigint {
  return getLib().symbols.nanos_to_micros(nanos) as bigint;
}

/** Converts a UNIX nanoseconds timestamp to an ISO 8601 (RFC 3339) string. */
export function unixNanosToIso8601(timestampNs: bigint): string {
  const cstrPtr = getLib().symbols.unix_nanos_to_iso8601_cstr(timestampNs);
  return readCStr(cstrPtr as number);
}

/** Converts a UNIX nanoseconds timestamp to an ISO 8601 string with millisecond precision. */
export function unixNanosToIso8601Millis(timestampNs: bigint): string {
  const cstrPtr = getLib().symbols.unix_nanos_to_iso8601_millis_cstr(timestampNs);
  return readCStr(cstrPtr as number);
}
