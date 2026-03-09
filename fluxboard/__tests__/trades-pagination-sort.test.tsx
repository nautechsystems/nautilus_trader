import { describe, it, expect, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { useEffect } from 'react';
import Trades from '../Trades';

const { socketMock, getTrades, getTradesDelta } = vi.hoisted(() => {
  const socketMock = { on: vi.fn(), off: vi.fn(), connected: true };
  return {
    socketMock,
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
  };
});

vi.mock('../sockets', () => ({ socket: socketMock }));
vi.mock('../api', () => ({ api: { getTrades, getTradesDelta } }));

const tableMock = vi.fn((props: any) => {
  useEffect(() => {
    props.onTimeSortChange?.('ts_asc');
  }, [props.onTimeSortChange]);
  return <div data-testid="table" />;
});

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: (props: any) => tableMock(props),
}));

describe('Trades sorting toggle triggers fetch with ts_asc', () => {
  it('toggles sort and re-fetches with ts_asc', async () => {
    getTrades.mockResolvedValue({ rows: [], total: 0, last_seq: 0, page: 1, page_size: 100 });
    getTradesDelta.mockResolvedValue({ rows: [], last_seq: 0, reset_required: false });

    render(<Trades />);
    await waitFor(() => {
      expect(getTrades.mock.calls.some((call) => call[2]?.sort === 'ts_asc')).toBe(true);
    });
  });
});
