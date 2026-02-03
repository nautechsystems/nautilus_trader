import { describe, expect, test, afterEach } from "bun:test";
import { Symbol } from "../../src/model/identifiers/symbol";
import { Venue } from "../../src/model/identifiers/venue";
import { TraderId } from "../../src/model/identifiers/trader_id";
import { AccountId } from "../../src/model/identifiers/account_id";
import { ClientId } from "../../src/model/identifiers/client_id";
import { ClientOrderId } from "../../src/model/identifiers/client_order_id";
import { StrategyId } from "../../src/model/identifiers/strategy_id";
import { InstrumentId } from "../../src/model/identifiers/instrument_id";
import { TradeId } from "../../src/model/identifiers/trade_id";

describe("Identifiers", () => {
  test("Symbol.from creates a symbol", () => {
    const sym = Symbol.from("BTCUSDT");
    expect(sym._ptr).not.toBe(0);
  });

  test("Symbol.hash is consistent", () => {
    const a = Symbol.from("BTCUSDT");
    const b = Symbol.from("BTCUSDT");
    expect(a.hash()).toBe(b.hash());
  });

  test("Symbol.hash differs for different values", () => {
    const a = Symbol.from("BTCUSDT");
    const b = Symbol.from("ETHUSDT");
    expect(a.hash()).not.toBe(b.hash());
  });

  test("Symbol.toString returns full value", () => {
    const sym = Symbol.from("BTCUSDT");
    expect(sym.toString()).toBe("BTCUSDT");
  });

  test("Symbol.toString works for composite symbols", () => {
    const sym = Symbol.from("CL.FUT");
    expect(sym.toString()).toBe("CL.FUT");
    expect(sym.root()).toBe("CL");
    expect(sym.isComposite()).toBe(true);
  });

  test("Venue.from creates a venue", () => {
    const venue = Venue.from("BINANCE");
    expect(venue._ptr).not.toBe(0);
  });

  test("Venue.hash is consistent", () => {
    const a = Venue.from("BINANCE");
    const b = Venue.from("BINANCE");
    expect(a.hash()).toBe(b.hash());
  });

  test("Venue.toString returns venue name", () => {
    const venue = Venue.from("BINANCE");
    expect(venue.toString()).toBe("BINANCE");
  });

  test("TraderId.from creates a trader id", () => {
    const id = TraderId.from("TRADER-001");
    expect(id._ptr).not.toBe(0);
  });

  test("AccountId.from creates an account id", () => {
    const id = AccountId.from("SIM-000");
    expect(id._ptr).not.toBe(0);
  });

  test("ClientId.from creates a client id", () => {
    const id = ClientId.from("CLIENT-001");
    expect(id._ptr).not.toBe(0);
  });

  test("ClientOrderId.from creates a client order id", () => {
    const id = ClientOrderId.from("O-20210101-000001");
    expect(id._ptr).not.toBe(0);
  });

  test("StrategyId.from creates a strategy id", () => {
    const id = StrategyId.from("S-001");
    expect(id._ptr).not.toBe(0);
  });

  test("InstrumentId.from creates from string", () => {
    const id = InstrumentId.from("BTCUSDT.BINANCE");
    expect(id.toString()).toBe("BTCUSDT.BINANCE");
    id.close();
  });

  test("InstrumentId.hash is consistent", () => {
    const a = InstrumentId.from("BTCUSDT.BINANCE");
    const b = InstrumentId.from("BTCUSDT.BINANCE");
    expect(a.hash()).toBe(b.hash());
    a.close();
    b.close();
  });

  test("InstrumentId.close is idempotent", () => {
    const id = InstrumentId.from("BTCUSDT.BINANCE");
    id.close();
    id.close(); // Should not crash
  });

  test("TradeId.from creates and round-trips", () => {
    const id = TradeId.from("TRADE-001");
    expect(id.toString()).toBe("TRADE-001");
    id.close();
  });

  test("TradeId.hash is consistent", () => {
    const a = TradeId.from("TRADE-001");
    const b = TradeId.from("TRADE-001");
    expect(a.hash()).toBe(b.hash());
    a.close();
    b.close();
  });
});
