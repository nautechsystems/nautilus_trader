import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, act, cleanup } from '@testing-library/react';
import Trades from '../Trades';

const { socketMock, getTrades, getTradesDelta } = vi.hoisted(() => {
  const socketMock = {
    on: vi.fn(),
    off: vi.fn(),
    connected: true,
  };
  return {
    socketMock,
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
  };
});

vi.mock('../sockets', () => ({ socket: socketMock }));

vi.mock('../api', () => ({
  api: {
    getTrades,
    getTradesDelta,
  },
}));

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: () => <div data-testid="table" />,
}));

describe('Trades delta reset behavior', () => {
  beforeEach(() => {
    getTrades.mockReset();
    getTradesDelta.mockReset();
    getTrades.mockResolvedValue({ rows: [], total: 0, last_seq: 10, page: 1, page_size: 100 });
    // First delta triggers reset, next deltas empty
    getTradesDelta
      .mockResolvedValueOnce({ rows: [], last_seq: 10, reset_required: true })
      .mockResolvedValue({ rows: [], last_seq: 10, reset_required: false });
  });

  afterEach(() => {
    cleanup();
  });

  it('fetches snapshot again when reset_required', async () => {
    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    // allow poller to run (~1s)
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1200));
    });

    // After reset_required, a second snapshot fetch should happen
    await waitFor(() => {
      expect(getTrades).toHaveBeenCalledTimes(2);
    });
  });
});
