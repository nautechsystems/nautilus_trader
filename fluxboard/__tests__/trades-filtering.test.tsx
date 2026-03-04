import { describe, it, expect, vi } from 'vitest';
import { useEffect } from 'react';
import { render, screen, waitFor } from '@testing-library/react';
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

// Mock TableFilter to immediately call onFilterChange
vi.mock('../components/shared/TableFilter', () => ({
  TableFilter: ({ onFilterChange }: any) => {
    useEffect(() => {
      onFilterChange({ coin: 'PLUME' });
    }, [onFilterChange]);
    return <div data-testid="filter" />;
  },
}));

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: () => <div data-testid="table" />,
}));

describe('Trades filtering fetches page 1 with filters', () => {
  it('applies filter and fetches snapshot with coin filter', async () => {
    getTrades.mockResolvedValue({ rows: [], total: 0, last_seq: 0, page: 1, page_size: 100 });
    getTradesDelta.mockResolvedValue({ rows: [], last_seq: 0, reset_required: false });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(2));

    const call = getTrades.mock.calls[getTrades.mock.calls.length - 1];
    expect(call[0]).toBe(1); // page 1
    expect(call[2]).toMatchObject({ coin: 'PLUME' });
  });
});
