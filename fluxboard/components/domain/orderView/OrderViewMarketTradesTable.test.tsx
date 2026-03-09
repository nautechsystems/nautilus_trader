import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { OrderViewMarketTradesTable } from './OrderViewMarketTradesTable';

describe('OrderViewMarketTradesTable', () => {
  it('re-pins auto-scroll on content updates even when row count is unchanged', async () => {
    const firstRows = [
      { trade_id: 't1', ts_ms: 1_700_000_000_000, side: 'buy', price: '30000', qty: '0.2' },
      { trade_id: 't2', ts_ms: 1_700_000_000_001, side: 'sell', price: '30001', qty: '0.3' },
    ];
    const nextRows = [
      { trade_id: 't3', ts_ms: 1_700_000_000_002, side: 'buy', price: '30002', qty: '0.2' },
      { trade_id: 't4', ts_ms: 1_700_000_000_003, side: 'sell', price: '30003', qty: '0.3' },
    ];

    const { rerender } = render(<OrderViewMarketTradesTable rows={firstRows as any} autoScroll />);
    const container = screen
      .getByTestId('order-view-market-trades-table')
      .querySelector('.overflow-auto');
    expect(container).toBeTruthy();
    container!.scrollTop = 140;

    rerender(<OrderViewMarketTradesTable rows={nextRows as any} autoScroll />);
    await waitFor(() => {
      expect(container!.scrollTop).toBe(0);
    });
  });
});
