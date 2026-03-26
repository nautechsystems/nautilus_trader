import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/react';
import Trades from '../Trades';

const { socketMock, getTrades, getTradesDelta, trackLastHandler } = vi.hoisted(() => {
  let lastHandler: any = null;
  const socketMock = {
    on: vi.fn((event: string, handler: any) => {
      if (event === 'trade_update') lastHandler = handler;
    }),
    off: vi.fn((event: string, handler: any) => {
      if (event === 'trade_update') expect(handler).toBe(lastHandler);
    }),
    connected: true,
  };
  return {
    socketMock,
    getTrades: vi.fn(() => Promise.resolve({ rows: [], total: 0, page: 1, page_size: 100, last_seq: 0 })),
    getTradesDelta: vi.fn(() => Promise.resolve({ rows: [], last_seq: 0, reset_required: false })),
    trackLastHandler: () => lastHandler,
  };
});

vi.mock('../sockets', () => ({
  socket: socketMock,
  standardSocketClient: {
    subscribe: vi.fn(() => () => {}),
  },
}));

vi.mock('../api', () => ({
  api: {
    getTrades,
    getTradesDelta,
  },
}));

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: () => <div data-testid="table" />,
}));

describe('Trades socket cleanup', () => {
  it('registers trade_update and cleans up with same handler reference', () => {
    const { unmount } = render(<Trades />);
    expect(socketMock.on).toHaveBeenCalledWith('trade_update', expect.any(Function));
    expect(trackLastHandler()).toBeTruthy();
    unmount();
    expect(socketMock.off).toHaveBeenCalledWith('trade_update', trackLastHandler());
  });
});
