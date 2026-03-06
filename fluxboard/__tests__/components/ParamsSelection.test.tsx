/**
 * Params selection interactions tests
 *
 * Covers single-click, shift-range, ctrl/cmd toggle, and drag behaviors.
 * Also guards against the bug where a simple click + slight pointer move
 * accidentally selects two rows (drag should only extend selection when Shift held).
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import Params from '../../Params';
import * as api from '../../api';

// Mock API module
vi.mock('../../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
    updateParams: vi.fn(),
  },
}));

// Use real params store (do NOT mock stores) so selection state works end-to-end

// Mock polling to avoid timers and background calls
vi.mock('../../hooks/index', () => ({
  usePolling: vi.fn(),
}));

// Silence toasts in tests
vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe('Params selection interactions', () => {
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
      qty: {
        key: 'qty',
        label: 'Quantity',
        description: 'Trade quantity',
        type: 'float' as const,
        default: 1.0,
        min_value: 0,
        max_value: 1000,
        step: 0.1,
        unit: 'units',
      },
    },
    deprecated: {},
  };

  const mockParams = [
    {
      strategy_id: 'alpha',
      running: true,
      params: { bot_on: '1', qty: '10' },
    },
    {
      strategy_id: 'bravo',
      running: false,
      params: { bot_on: '0', qty: '20' },
    },
    {
      strategy_id: 'charlie',
      running: true,
      params: { bot_on: '1', qty: '30' },
    },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema as any);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);
  });

  function getTableRows() {
    const table = screen.getByRole('table');
    return table.querySelectorAll('tbody tr');
  }

  function getStrategyCell(rowEl: Element): HTMLElement {
    // First cell is the sticky strategy cell
    const cell = rowEl.querySelectorAll('td')[0] as HTMLElement | undefined;
    if (!cell) throw new Error('Strategy cell not found');
    return cell;
  }

  async function renderAndWait() {
    render(<Params />);
    await waitFor(() => {
      expect(screen.getByText('alpha')).toBeInTheDocument();
    });
  }

  function selectedCountText() {
    // Two areas show selected count; find any visible label like "X selected"
    // Use queryByText with regex to match "<number> selected"
    return screen.queryAllByText(/\d+ selected/).map((n) => n.textContent);
  }

  it('single click selects only one row', async () => {
    await renderAndWait();

    const rows = getTableRows();
    const alphaCell = getStrategyCell(rows[0]);

    fireEvent.mouseDown(alphaCell, { button: 0 });
    fireEvent.mouseUp(window);

    // Expect exactly 1 selected
    const labels = selectedCountText();
    expect(labels.some((t) => t === '1 selected')).toBe(true);
    expect(labels.some((t) => t === '2 selected')).toBe(false);
  });

  it('ctrl/cmd click toggles selection add/remove', async () => {
    await renderAndWait();

    const rows = getTableRows();
    const alphaCell = getStrategyCell(rows[0]);
    const bravoCell = getStrategyCell(rows[1]);

    // Select alpha normally
    fireEvent.mouseDown(alphaCell, { button: 0 });
    fireEvent.mouseUp(window);
    expect(selectedCountText().some((t) => t === '1 selected')).toBe(true);

    // Add bravo with ctrl (or meta)
    fireEvent.mouseDown(bravoCell, { button: 0, ctrlKey: true });
    fireEvent.mouseUp(window);
    expect(selectedCountText().some((t) => t === '2 selected')).toBe(true);

    // Remove alpha with ctrl
    fireEvent.mouseDown(alphaCell, { button: 0, ctrlKey: true });
    fireEvent.mouseUp(window);
    const labels = selectedCountText();
    expect(labels.some((t) => t === '1 selected')).toBe(true);
    expect(labels.some((t) => t === '2 selected')).toBe(false);
  });

  it('shift+click selects range from anchor', async () => {
    await renderAndWait();

    const rows = getTableRows();
    const alphaCell = getStrategyCell(rows[0]);
    const charlieCell = getStrategyCell(rows[2]);

    // Click alpha to set anchor
    fireEvent.mouseDown(alphaCell, { button: 0 });
    fireEvent.mouseUp(window);

    // Shift+click charlie -> expect 3 selected
    fireEvent.mouseDown(charlieCell, { button: 0, shiftKey: true });
    fireEvent.mouseUp(window);
    expect(selectedCountText().some((t) => t === '3 selected')).toBe(true);
  });

  it('does not expand selection on drag without Shift (bug guard)', async () => {
    await renderAndWait();

    const rows = getTableRows();
    const alphaCell = getStrategyCell(rows[0]);
    const bravoCell = getStrategyCell(rows[1]);

    // Mouse down on alpha (no modifiers)
    fireEvent.mouseDown(alphaCell, { button: 0 });

    // Slight drag entering next row without shift should NOT expand selection
    fireEvent.mouseEnter(bravoCell);
    fireEvent.mouseUp(window);

    const labels = selectedCountText();
    expect(labels.some((t) => t === '1 selected')).toBe(true);
    expect(labels.some((t) => t === '2 selected')).toBe(false);
  });

  it('allows range drag when holding Shift', async () => {
    await renderAndWait();

    const rows = getTableRows();
    const alphaCell = getStrategyCell(rows[0]);
    const charlieCell = getStrategyCell(rows[2]);

    // Start drag with Shift on alpha
    fireEvent.mouseDown(alphaCell, { button: 0, shiftKey: true });
    // Drag over charlie's row
    fireEvent.mouseEnter(charlieCell);
    fireEvent.mouseUp(window);

    expect(selectedCountText().some((t) => t === '3 selected')).toBe(true);
  });

});
