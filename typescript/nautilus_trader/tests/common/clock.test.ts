import { describe, expect, test, afterEach } from "bun:test";
import { TestClock, LiveClock } from "../../src/common/clock";

describe("TestClock", () => {
  let clock: TestClock;

  afterEach(() => {
    if (clock) clock.close();
  });

  test("new creates a clock at time zero", () => {
    clock = new TestClock();
    expect(clock.timestampNs()).toBe(0n);
  });

  test("setTime advances the clock", () => {
    clock = new TestClock();
    const timeNs = 1_000_000_000n; // 1 second
    clock.setTime(timeNs);
    expect(clock.timestampNs()).toBe(timeNs);
  });

  test("timestamp returns seconds as float", () => {
    clock = new TestClock();
    clock.setTime(1_500_000_000_000_000_000n); // 1.5 billion seconds in ns
    const ts = clock.timestamp();
    expect(ts).toBeGreaterThan(0);
  });

  test("timestampMs returns milliseconds", () => {
    clock = new TestClock();
    clock.setTime(1_000_000_000n); // 1 second
    expect(clock.timestampMs()).toBe(1000n);
  });

  test("timestampUs returns microseconds", () => {
    clock = new TestClock();
    clock.setTime(1_000_000_000n); // 1 second
    expect(clock.timestampUs()).toBe(1_000_000n);
  });

  test("timerCount starts at zero", () => {
    clock = new TestClock();
    expect(clock.timerCount()).toBe(0);
  });

  test("timerNames starts empty", () => {
    clock = new TestClock();
    expect(clock.timerNames()).toEqual([]);
  });

  test("close is idempotent", () => {
    clock = new TestClock();
    clock.close();
    clock.close(); // Should not crash
    clock = null!;
  });
});

describe("LiveClock", () => {
  let clock: LiveClock;

  afterEach(() => {
    if (clock) clock.close();
  });

  test("new creates a live clock", () => {
    clock = new LiveClock();
    const ts = clock.timestampNs();
    expect(ts).toBeGreaterThan(0n);
  });

  test("timestamp returns current time", () => {
    clock = new LiveClock();
    const ts = clock.timestamp();
    // Should be somewhere after 2024
    expect(ts).toBeGreaterThan(1700000000);
  });

  test("timestampMs returns current time in ms", () => {
    clock = new LiveClock();
    const ms = clock.timestampMs();
    expect(ms).toBeGreaterThan(1700000000000n);
  });

  test("timerCount starts at zero", () => {
    clock = new LiveClock();
    expect(clock.timerCount()).toBe(0);
  });
});
