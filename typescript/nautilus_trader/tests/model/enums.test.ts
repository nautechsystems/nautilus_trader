import { describe, expect, test } from "bun:test";
import {
  OrderSide,
  BookAction,
  AccountType,
  OrderStatus,
  OrderType,
  PositionSide,
  TimeInForce,
  PriceType,
  TriggerType,
  TradingState,
  AssetClass,
  InstrumentClass,
  RecordFlag,
  MarketStatus,
} from "../../src/model/enums";

describe("Enums", () => {
  test("OrderSide has correct discriminant values", () => {
    expect(OrderSide.NO_ORDER_SIDE).toBe(0);
    expect(OrderSide.BUY).toBe(1);
    expect(OrderSide.SELL).toBe(2);
  });

  test("BookAction has correct discriminant values", () => {
    expect(BookAction.ADD).toBe(1);
    expect(BookAction.UPDATE).toBe(2);
    expect(BookAction.DELETE).toBe(3);
    expect(BookAction.CLEAR).toBe(4);
  });

  test("AccountType has correct discriminant values", () => {
    expect(AccountType.CASH).toBe(1);
    expect(AccountType.MARGIN).toBe(2);
    expect(AccountType.BETTING).toBe(3);
    expect(AccountType.WALLET).toBe(4);
  });

  test("OrderStatus has correct discriminant values", () => {
    expect(OrderStatus.INITIALIZED).toBe(1);
    expect(OrderStatus.DENIED).toBe(2);
    expect(OrderStatus.SUBMITTED).toBe(5);
    expect(OrderStatus.ACCEPTED).toBe(6);
    expect(OrderStatus.CANCELED).toBe(8);
    expect(OrderStatus.FILLED).toBe(14);
  });

  test("OrderType has correct discriminant values", () => {
    expect(OrderType.MARKET).toBe(1);
    expect(OrderType.LIMIT).toBe(2);
    expect(OrderType.STOP_MARKET).toBe(3);
    expect(OrderType.STOP_LIMIT).toBe(4);
  });

  test("PositionSide has correct discriminant values", () => {
    expect(PositionSide.NO_POSITION_SIDE).toBe(0);
    expect(PositionSide.FLAT).toBe(1);
    expect(PositionSide.LONG).toBe(2);
    expect(PositionSide.SHORT).toBe(3);
  });

  test("TimeInForce has correct discriminant values", () => {
    expect(TimeInForce.GTC).toBe(1);
    expect(TimeInForce.IOC).toBe(2);
    expect(TimeInForce.FOK).toBe(3);
    expect(TimeInForce.GTD).toBe(4);
    expect(TimeInForce.DAY).toBe(5);
  });

  test("PriceType has correct discriminant values", () => {
    expect(PriceType.BID).toBe(1);
    expect(PriceType.ASK).toBe(2);
    expect(PriceType.MID).toBe(3);
    expect(PriceType.LAST).toBe(4);
    expect(PriceType.MARK).toBe(5);
  });

  test("TriggerType has correct discriminant values", () => {
    expect(TriggerType.NO_TRIGGER).toBe(0);
    expect(TriggerType.DEFAULT).toBe(1);
    expect(TriggerType.LAST_PRICE).toBe(2);
    expect(TriggerType.BID_ASK).toBe(5);
    expect(TriggerType.MID_POINT).toBe(9);
  });

  test("TradingState has correct discriminant values", () => {
    expect(TradingState.ACTIVE).toBe(1);
    expect(TradingState.HALTED).toBe(2);
    expect(TradingState.REDUCING).toBe(3);
  });

  test("AssetClass has correct discriminant values", () => {
    expect(AssetClass.FX).toBe(1);
    expect(AssetClass.EQUITY).toBe(2);
    expect(AssetClass.CRYPTOCURRENCY).toBe(6);
  });

  test("InstrumentClass has correct discriminant values", () => {
    expect(InstrumentClass.SPOT).toBe(1);
    expect(InstrumentClass.FUTURE).toBe(3);
    expect(InstrumentClass.OPTION).toBe(8);
    expect(InstrumentClass.BINARY_OPTION).toBe(12);
  });

  test("RecordFlag has correct bit field values", () => {
    expect(RecordFlag.F_LAST).toBe(128);  // 1 << 7
    expect(RecordFlag.F_TOB).toBe(64);    // 1 << 6
    expect(RecordFlag.F_SNAPSHOT).toBe(32); // 1 << 5
    expect(RecordFlag.F_MBP).toBe(16);    // 1 << 4
  });

  test("MarketStatus has correct discriminant values (non-contiguous)", () => {
    expect(MarketStatus.OPEN).toBe(1);
    expect(MarketStatus.CLOSED).toBe(2);
    expect(MarketStatus.PAUSED).toBe(3);
    expect(MarketStatus.SUSPENDED).toBe(5); // Note: 4 is skipped
    expect(MarketStatus.NOT_AVAILABLE).toBe(6);
  });
});
