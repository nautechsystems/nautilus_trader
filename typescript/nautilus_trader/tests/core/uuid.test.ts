import { describe, expect, test, afterEach } from "bun:test";
import { UUID4 } from "../../src/core/uuid";

describe("UUID4", () => {
  const uuids: UUID4[] = [];
  afterEach(() => {
    for (const u of uuids) u.close();
    uuids.length = 0;
  });

  function track(u: UUID4): UUID4 {
    uuids.push(u);
    return u;
  }

  test("create generates a valid UUID4 string", () => {
    const uuid = track(UUID4.create());
    const str = uuid.toString();
    expect(str).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
  });

  test("fromString round-trips correctly", () => {
    const original = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
    const uuid = track(UUID4.fromString(original));
    expect(uuid.toString()).toBe(original);
  });

  test("equals returns true for identical UUIDs", () => {
    const a = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c757"));
    const b = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c757"));
    expect(a.equals(b)).toBe(true);
  });

  test("equals returns false for different UUIDs", () => {
    const a = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c757"));
    const b = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c758"));
    expect(a.equals(b)).toBe(false);
  });

  test("hash is consistent for same UUID", () => {
    const a = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c757"));
    const b = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c757"));
    expect(a.hash()).toBe(b.hash());
  });

  test("hash differs for different UUIDs", () => {
    const a = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c757"));
    const b = track(UUID4.fromString("2d89666b-1a1e-4a75-b193-4eb3b454c758"));
    expect(a.hash()).not.toBe(b.hash());
  });

  test("create generates unique UUIDs", () => {
    const a = track(UUID4.create());
    const b = track(UUID4.create());
    expect(a.toString()).not.toBe(b.toString());
  });

  test("close is idempotent", () => {
    const uuid = UUID4.create();
    uuid.close();
    uuid.close(); // Should not crash
  });
});
