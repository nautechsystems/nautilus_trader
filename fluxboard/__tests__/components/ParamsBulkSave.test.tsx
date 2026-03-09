/**
 * Params - Bulk Save flows
 *
 * Covers:
 * - Save All dispatches bulk PATCH with correct updates
 * - Save Selected only saves selected dirty strategies
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
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

// Silence toasts
vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe('Params - Bulk Save', () => {
  const mockSchema = {
    params: {
      qty: { key: 'qty', label: 'qty', description: '', type: 'float', default: 1.0, min_value: 0.0, max_value: 1000.0 },
      cooldown: { key: 'cooldown', label: 'cooldown', description: '', type: 'float', default: 10.0, min_value: 0.0, max_value: 300.0 },
      bot_on: { key: 'bot_on', label: 'bot_on', description: '', type: 'select', default: '0', options: [['0','Off'],['1','On']] },
    },
    deprecated: {},
  } as any;

  const mockParams = [
    { strategy_id: 'alpha', running: true, params: { bot_on: '1', qty: '10', cooldown: '10.0' } },
    { strategy_id: 'bravo', running: false, params: { bot_on: '0', qty: '20', cooldown: '10.0' } },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);
    vi.mocked(api.api.updateParams).mockResolvedValue({ success: 2, failed: 0, errors: [] } as any);
  });

  it('Save All sends bulk updates for all dirty strategies', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    // Change alpha qty
    fireEvent.change(screen.getByDisplayValue('10'), { target: { value: '12' } });
    // Change bravo cooldown
    fireEvent.change(screen.getAllByDisplayValue('10.0')[1], { target: { value: '11.5' } });

    // Click Save All in header
    fireEvent.click(screen.getByRole('button', { name: /Save All/ }));

    await waitFor(() => expect(api.api.updateParams).toHaveBeenCalled());
    const [updates] = vi.mocked(api.api.updateParams).mock.calls[0];
    // We expect two entries
    expect(updates.length).toBe(2);
    const map = new Map(updates.map((u: any) => [u.strategy_id, u.params]));
    expect(map.get('alpha')).toEqual({ qty: '12' });
    expect(map.get('bravo')).toEqual({ cooldown: '11.5' });
  });

  it('Save Selected saves only selection', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    // Select only alpha (click its strategy cell)
    const rows = screen.getByRole('table').querySelectorAll('tbody tr');
    const alphaCell = rows[0].querySelectorAll('td')[0] as HTMLElement;
    fireEvent.mouseDown(alphaCell, { button: 0 });
    fireEvent.mouseUp(window);

    // Change both alpha and bravo (only alpha should be saved when using Save Selected)
    fireEvent.change(screen.getByDisplayValue('10'), { target: { value: '13' } });
    fireEvent.change(screen.getAllByDisplayValue('10.0')[1], { target: { value: '9.0' } });

    // Click Save Selected in toolbar
    fireEvent.click(screen.getByRole('button', { name: /Save Selected/ }));

    await waitFor(() => expect(api.api.updateParams).toHaveBeenCalled());
    const [updates] = vi.mocked(api.api.updateParams).mock.calls[0];
    expect(updates.length).toBe(1);
    expect(updates[0]).toEqual({ strategy_id: 'alpha', params: { qty: '13' } });
  });
});

