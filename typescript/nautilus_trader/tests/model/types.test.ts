import { describe, expect, test } from "bun:test";
import { Price } from "../../src/model/types/price";
import { Quantity } from "../../src/model/types/quantity";
import { Currency } from "../../src/model/types/currency";
import { Money } from "../../src/model/types/money";

describe("Price", () => {
  test("fromFloat creates a price and round-trips to float", () => {
    const price = Price.fromFloat(100.25, 2);
    expect(Math.abs(price.asFloat() - 100.25)).toBeLessThan(0.001);
    price.close();
  });

  test("fromFloat with higher precision", () => {
    const price = Price.fromFloat(1.23456789, 8);
    expect(Math.abs(price.asFloat() - 1.23456789)).toBeLessThan(0.0000001);
    price.close();
  });

  test("precision returns the correct value", () => {
    const price = Price.fromFloat(42.5, 3);
    expect(price.precision).toBe(3);
    price.close();
  });

  test("toString returns formatted string", () => {
    const price = Price.fromFloat(100.5, 2);
    expect(price.toString()).toMatch(/100\.50/);
    price.close();
  });

  test("close is idempotent", () => {
    const price = Price.fromFloat(1.0, 1);
    price.close();
    price.close(); // Should not crash
  });
});

describe("Quantity", () => {
  test("fromFloat creates a quantity and round-trips to float", () => {
    const qty = Quantity.fromFloat(10.5, 1);
    expect(Math.abs(qty.asFloat() - 10.5)).toBeLessThan(0.01);
    qty.close();
  });

  test("fromFloat with zero precision", () => {
    const qty = Quantity.fromFloat(100.0, 0);
    expect(qty.asFloat()).toBe(100.0);
    qty.close();
  });

  test("precision returns the correct value", () => {
    const qty = Quantity.fromFloat(5.0, 2);
    expect(qty.precision).toBe(2);
    qty.close();
  });

  test("close is idempotent", () => {
    const qty = Quantity.fromFloat(1.0, 0);
    qty.close();
    qty.close(); // Should not crash
  });
});

describe("Currency", () => {
  test("from creates a currency", () => {
    const usd = Currency.from("USD");
    expect(usd.code()).toBe("USD");
    usd.close();
  });

  test("precision returns correct value", () => {
    const usd = Currency.from("USD");
    expect(usd.precision).toBe(2);
    usd.close();
  });

  test("exists returns true for known currencies", () => {
    expect(Currency.exists("USD")).toBe(true);
    expect(Currency.exists("BTC")).toBe(true);
  });

  test("close is idempotent", () => {
    const usd = Currency.from("USD");
    usd.close();
    usd.close(); // Should not crash
  });
});

describe("Money", () => {
  test("create and round-trip to float", () => {
    const usd = Currency.from("USD");
    const money = Money.create(100.5, usd);
    expect(Math.abs(money.asFloat() - 100.5)).toBeLessThan(0.01);
    money.close();
    usd.close();
  });

  test("close is idempotent", () => {
    const usd = Currency.from("USD");
    const money = Money.create(50.0, usd);
    money.close();
    money.close(); // Should not crash
    usd.close();
  });
});
