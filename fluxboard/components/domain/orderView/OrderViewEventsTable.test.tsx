import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { OrderViewEventsTable } from './OrderViewEventsTable';

const neutralFocus = { orderKey: null, eventKey: null, side: null, price: null } as const;

describe('OrderViewEventsTable', () => {
  it('re-pins auto-scroll on content updates even when row count is unchanged', async () => {
    const firstRows = [
      {
        event_key: 'evt-1',
        ts_ms: 1_700_000_000_000,
        type: 'quote.placed',
        side: 'bid',
        px: '30000',
      },
      {
        event_key: 'evt-2',
        ts_ms: 1_700_000_000_001,
        type: 'quote.placed',
        side: 'ask',
        px: '30001',
      },
    ];
    const nextRows = [
      {
        event_key: 'evt-3',
        ts_ms: 1_700_000_000_002,
        type: 'quote.cancel',
        side: 'bid',
        px: '30000',
      },
      {
        event_key: 'evt-4',
        ts_ms: 1_700_000_000_003,
        type: 'quote.cancel',
        side: 'ask',
        px: '30001',
      },
    ];

    const { rerender } = render(
      <OrderViewEventsTable rows={firstRows as any} autoScroll focus={neutralFocus} />
    );
    const container = screen.getByTestId('order-view-events-table').querySelector('.overflow-auto');
    expect(container).toBeTruthy();
    container!.scrollTop = 120;

    rerender(<OrderViewEventsTable rows={nextRows as any} autoScroll focus={neutralFocus} />);
    await waitFor(() => {
      expect(container!.scrollTop).toBe(0);
    });
  });
});
