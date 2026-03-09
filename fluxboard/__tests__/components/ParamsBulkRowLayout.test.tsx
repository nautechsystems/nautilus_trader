/**
 * Params - Bulk row layout and stickiness
 * Ensures only one bulk row renders in tbody and it stays pinned directly under the header.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
    updateParams: vi.fn(),
  },
}));

vi.mock('../../hooks', () => ({
  usePolling: vi.fn(),
}));

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe('Params bulk row layout', () => {
  const mockSchema = {
    params: {
      bot_on: {
        key: 'bot_on',
        label: 'Bot On',
        description: 'Enable trading',
        type: 'select' as const,
        default: '0',
        options: [
          ['0', 'Off'],
          ['1', 'On'],
        ] as [string, string][],
        unit: null,
      },
      cex_bid_edge: {
        key: 'cex_bid_edge',
        label: 'Bid Edge',
        description: 'Bid edge threshold',
        type: 'float' as const,
        default: 1.0,
        min_value: 0,
        max_value: 100,
        step: 0.1,
        unit: 'bps',
      },
    },
    deprecated: {},
  };

  const mockParams = [
    { strategy_id: 'futu_hl_a', running: true, params: { bot_on: '1', cex_bid_edge: '5' } },
    { strategy_id: 'futu_hl_b', running: true, params: { bot_on: '1', cex_bid_edge: '6' } },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema as any);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);

    // Mock ResizeObserver to report a taller header height (38px) so we can
    // verify the bulk row uses the measured height instead of a hard-coded value.
    class MockResizeObserver {
      callback: ResizeObserverCallback;
      constructor(cb: ResizeObserverCallback) {
        this.callback = cb;
      }
      observe(target: Element) {
        // Immediately invoke with a measured height of 38px.
        this.callback([
          {
            target,
            contentRect: {
              width: 0,
              height: 38,
              x: 0,
              y: 0,
              top: 0,
              left: 0,
              right: 0,
              bottom: 0,
              toJSON: () => ({})
            } as DOMRectReadOnly,
            borderBoxSize: [],
            contentBoxSize: [],
            devicePixelContentBoxSize: []
          } as ResizeObserverEntry
        ], {} as ResizeObserver);
      }
      unobserve() {}
      disconnect() {}
    }

    // @ts-expect-error allow jsdom override
    global.ResizeObserver = MockResizeObserver as any;
  });

  it('renders a single sticky bulk row pinned below the header', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('futu_hl_a')).toBeInTheDocument();
    });

    const headerRow = screen.getByTestId('params-header-row');
    const headerStyle = headerRow.getAttribute('style') || '';
    expect(headerRow.tagName).toBe('TR');
    expect(headerStyle).toMatch(/position:\s*sticky/i);
    expect(headerStyle).toMatch(/top:\s*0/);

    const bulkRow = await screen.findByTestId('bulk-row');
    expect(bulkRow.tagName).toBe('TR');

    await waitFor(() => {
      const bulkStyle = bulkRow.getAttribute('style') || '';
      expect(bulkStyle).toMatch(/position:\s*sticky/i);
      expect(bulkStyle).toMatch(/top:\s*38px/);
      expect(bulkStyle).toMatch(/background-color/);
    });
  });
});
