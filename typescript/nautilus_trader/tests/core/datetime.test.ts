import { describe, expect, test } from "bun:test";
import {
  secsToNanos,
  secsToMillis,
  millisToNanos,
  microsToNanos,
  nanosToSecs,
  nanosToMillis,
  nanosToMicros,
  unixNanosToIso8601,
} from "../../src/core/datetime";

describe("datetime conversions", () => {
  test("secsToNanos converts correctly", () => {
    expect(secsToNanos(1.0)).toBe(1_000_000_000n);
  });

  test("secsToMillis converts correctly", () => {
    expect(secsToMillis(1.0)).toBe(1000n);
  });

  test("millisToNanos converts correctly", () => {
    expect(millisToNanos(1.0)).toBe(1_000_000n);
  });

  test("microsToNanos converts correctly", () => {
    expect(microsToNanos(1.0)).toBe(1000n);
  });

  test("nanosToSecs converts correctly", () => {
    expect(nanosToSecs(1_000_000_000n)).toBe(1.0);
  });

  test("nanosToMillis converts correctly", () => {
    expect(nanosToMillis(1_000_000_000n)).toBe(1000n);
  });

  test("nanosToMicros converts correctly", () => {
    expect(nanosToMicros(1_000_000_000n)).toBe(1_000_000n);
  });

  test("unixNanosToIso8601 formats correctly", () => {
    // 2021-01-01T00:00:00Z = 1609459200 seconds = 1609459200000000000 ns
    const result = unixNanosToIso8601(1609459200000000000n);
    expect(result).toContain("2021-01-01");
  });

  test("round-trip secs -> nanos -> secs", () => {
    const secs = 1234567.89;
    const nanos = secsToNanos(secs);
    const back = nanosToSecs(nanos);
    expect(Math.abs(back - secs)).toBeLessThan(0.001);
  });
});
