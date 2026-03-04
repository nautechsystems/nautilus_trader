// Test helpers for socket simulation

import type { Socket } from 'socket.io-client';
import type { Trade, MarketSnapshot } from './types';

export const simulateSocketEvent = (
  socket: Socket,
  event: string,
  payload: unknown
) => {
  socket.emit(event, payload);
};

export const mockTradeUpdate: Trade = {
  time: '2025-10-18 12:34:56.78',
  coin: 'PLUME',
  exchange: 'bybit',
  side: 'buy',
  price: '0.1234',
  qty: '100.0',
  mv: '12.34',
  fee: '0.0037',
  exch_id: 'exch_123',
  trade_id: 'test_trade_123',
  signal_id: 'signal_789',
  order_id: 'test_order_456',
  notes: 'test'
};

export const mockMarketUpdate: MarketSnapshot = {
  symbol: 'PLUME/USDT',
  exchange: 'bybit',
  bid_qty: '1000',
  bid: '0.123',
  mid_px: '0.1235',
  ask: '0.124',
  ask_qty: '2000',
  timestamp_ms: Date.now(),
  update_time: '2025-10-18 12:34:56.78'
};
