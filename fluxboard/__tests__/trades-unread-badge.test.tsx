import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { useEffect } from 'react';
import Trades from '../Trades';

let tableProps: any = null;
const tableMock = vi.fn((props: any) => {
  tableProps = props;
  // Simulate user not at top so isViewingLatest becomes false
  useEffect(() => {
    props.onScrollStateChange?.({ atTop: false });
  }, [props.onScrollStateChange]);
  return <div data-testid="table" />;
});

const { socketHandlers, socketMock, getTrades, getTradesDelta } = vi.hoisted(() => {
  const socketHandlers: Record<string, (msg: any) => void> = {};
  const socketMock = {
    on: vi.fn((event: string, handler: any) => {
      socketHandlers[event] = handler;
    }),
    off: vi.fn((event: string) => delete socketHandlers[event]),
    connected: true,
  };
  return {
    socketHandlers,
    socketMock,
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
  };
});

vi.mock('../sockets', () => ({ socket: socketMock }));
vi.mock('../api', () => ({ api: { getTrades, getTradesDelta } }));

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: (props: any) => tableMock(props),
}));

describe('Trades unread badge behavior', () => {
  it('increments unread on live upsert when not viewing latest and clears on Jump to latest', async () => {
    getTrades.mockResolvedValue({ rows: [], total: 0, last_seq: 0, page: 1, page_size: 100 });
    getTradesDelta.mockResolvedValue({ rows: [], last_seq: 0, reset_required: false });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    // Push a live upsert while not viewing latest
    handler?.({ op: 'upsert', row_id: 'live', seq: 5, version: 1, ts: 5 });
    await waitFor(() => screen.getByRole('button', { name: /new/i }));

    // Jump to latest clears unread and triggers fetch
    const jumpBtn = screen.getByRole('button', { name: /Jump to latest/i });
    fireEvent.click(jumpBtn);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(2));
    // Unread badge should disappear
    expect(screen.queryByRole('button', { name: /new/i })).toBeNull();
  });
});
