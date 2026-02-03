/**
 * Clock wrapper classes delegating to the Rust FFI implementation.
 *
 * TestClock and LiveClock follow the "stateful opaque pointer" pattern:
 * they are heap-allocated on the Rust side via `_new()` and must be
 * explicitly freed with `close()` which calls `_drop()`.
 */
import { getLib } from "../lib";
import { readCStr, toCString } from "../_internal/memory";
import type { Pointer } from "../_internal/types";

/**
 * A clock for backtesting and unit testing which can be manually advanced.
 */
export class TestClock {
  private _ptr: Pointer;

  constructor() {
    this._ptr = getLib().symbols.bun_test_clock_new() as number;
  }

  /** Set the clock time to the given UNIX nanoseconds timestamp. */
  setTime(toTimeNs: bigint): void {
    getLib().symbols.test_clock_set_time(this._ptr, toTimeNs);
  }

  /** Get the current timestamp as seconds (f64). */
  timestamp(): number {
    return getLib().symbols.test_clock_timestamp(this._ptr) as number;
  }

  /** Get the current timestamp in milliseconds. */
  timestampMs(): bigint {
    return getLib().symbols.test_clock_timestamp_ms(this._ptr) as bigint;
  }

  /** Get the current timestamp in microseconds. */
  timestampUs(): bigint {
    return getLib().symbols.test_clock_timestamp_us(this._ptr) as bigint;
  }

  /** Get the current timestamp in nanoseconds. */
  timestampNs(): bigint {
    return getLib().symbols.test_clock_timestamp_ns(this._ptr) as bigint;
  }

  /** Get the timer names as an array of strings. */
  timerNames(): string[] {
    const cstrPtr = getLib().symbols.test_clock_timer_names(this._ptr);
    const joined = readCStr(cstrPtr as number);
    if (joined === "") return [];
    return joined.split("<,>");
  }

  /** Get the count of active timers. */
  timerCount(): number {
    return Number(getLib().symbols.test_clock_timer_count(this._ptr));
  }

  /** Get the next time for a named timer. */
  nextTime(name: string): bigint {
    return getLib().symbols.test_clock_next_time(this._ptr, toCString(name)) as bigint;
  }

  /** Cancel a named timer. */
  cancelTimer(name: string): void {
    getLib().symbols.test_clock_cancel_timer(this._ptr, toCString(name));
  }

  /** Cancel all timers. */
  cancelTimers(): void {
    getLib().symbols.test_clock_cancel_timers(this._ptr);
  }

  /** Drop the underlying Rust object and free memory. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_test_clock_drop(this._ptr);
      this._ptr = 0;
    }
  }
}

/**
 * A clock providing real-time timestamps from the system clock.
 */
export class LiveClock {
  private _ptr: Pointer;

  constructor() {
    this._ptr = getLib().symbols.bun_live_clock_new() as number;
  }

  /** Get the current timestamp as seconds (f64). */
  timestamp(): number {
    return getLib().symbols.live_clock_timestamp(this._ptr) as number;
  }

  /** Get the current timestamp in milliseconds. */
  timestampMs(): bigint {
    return getLib().symbols.live_clock_timestamp_ms(this._ptr) as bigint;
  }

  /** Get the current timestamp in microseconds. */
  timestampUs(): bigint {
    return getLib().symbols.live_clock_timestamp_us(this._ptr) as bigint;
  }

  /** Get the current timestamp in nanoseconds. */
  timestampNs(): bigint {
    return getLib().symbols.live_clock_timestamp_ns(this._ptr) as bigint;
  }

  /** Get the timer names as an array of strings. */
  timerNames(): string[] {
    const cstrPtr = getLib().symbols.live_clock_timer_names(this._ptr);
    const joined = readCStr(cstrPtr as number);
    if (joined === "") return [];
    return joined.split("<,>");
  }

  /** Get the count of active timers. */
  timerCount(): number {
    return Number(getLib().symbols.live_clock_timer_count(this._ptr));
  }

  /** Get the next time for a named timer. */
  nextTime(name: string): bigint {
    return getLib().symbols.live_clock_next_time(this._ptr, toCString(name)) as bigint;
  }

  /** Cancel a named timer. */
  cancelTimer(name: string): void {
    getLib().symbols.live_clock_cancel_timer(this._ptr, toCString(name));
  }

  /** Cancel all timers. */
  cancelTimers(): void {
    getLib().symbols.live_clock_cancel_timers(this._ptr);
  }

  /** Drop the underlying Rust object and free memory. */
  close(): void {
    if (this._ptr !== 0) {
      getLib().symbols.bun_live_clock_drop(this._ptr);
      this._ptr = 0;
    }
  }
}
