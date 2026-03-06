/**
 * Params - Edit and Save flows
 *
 * Covers:
 * - Initial load with schema + params
 * - Inline edit marks dirty and validates
 * - Row Save calls PATCH with only dirty fields
 * - Enter key triggers Save
 * - Validation failure focuses first invalid field
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent, act, within } from '@testing-library/react';
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
  usePolling: vi.fn((callback: () => void, _interval: number, _enabled: boolean) => {
    (globalThis as any).__paramsEditPolling = callback;
  }),
}));

// Silence toasts
vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe('Params - Edit and Save', () => {
  const mockSchema = {
    params: {
      bot_on: {
        key: 'bot_on', label: 'bot_on', description: '', type: 'select', default: '0',
        options: [['0','Off'], ['1','On']], unit: null
      },
      qty: {
        key: 'qty', label: 'qty', description: '', type: 'float', default: 1.0,
        min_value: 0.0, max_value: 1000.0, step: 0.1, unit: 'units'
      },
      max_age_ms: {
        key: 'max_age_ms', label: 'max_age', description: '', type: 'int', default: 2500,
        min_value: 100, max_value: 60000, step: 100, unit: 'milliseconds'
      },
      freshness_mode: {
        key: 'freshness_mode', label: 'freshness_mode', description: '', type: 'select', default: 'enforce',
        options: [['enforce','Enforce'], ['warn','Warn Only']], unit: null
      }
    },
    deprecated: {},
  } as any;

  const mockParams = [
    { strategy_id: 'alpha', running: true, params: { bot_on: '1', qty: '10', max_age_ms: '2500', freshness_mode: 'enforce' } },
    { strategy_id: 'bravo', running: false, params: { bot_on: '0', qty: '20', max_age_ms: '3000', freshness_mode: 'warn' } },
  ];

  function rowFor(strategyId: string) {
    const strategyButton = screen.getByRole('button', { name: strategyId });
    const row = strategyButton.closest('tr');
    if (!row) throw new Error('Strategy row not found');
    return row as HTMLElement;
  }

  async function triggerConflictUpdate(nextQty: string) {
    vi.mocked(api.api.getParams).mockResolvedValueOnce([
      { ...mockParams[0], params: { ...mockParams[0].params, qty: nextQty } },
      mockParams[1]
    ] as any);

    const callback = (globalThis as any).__paramsEditPolling;
    await act(async () => {
      await callback?.();
    });
  }

  async function triggerUnchangedUpdate() {
    // Return the same data the UI initially loaded to simulate no remote change
    vi.mocked(api.api.getParams).mockResolvedValueOnce(mockParams as any);

    const callback = (globalThis as any).__paramsEditPolling;
    await act(async () => {
      await callback?.();
    });
  }

  beforeEach(() => {
    vi.clearAllMocks();
    (globalThis as any).__paramsEditPolling = undefined;
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema as any);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);
  });

  afterEach(() => {
    delete (globalThis as any).__paramsEditPolling;
  });

  it('marks cell dirty and saves only dirty fields', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    // Change qty for alpha
    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '12.5' } });
    fireEvent.blur(qtyInput);

    // Click Save on row
    const saveButton = within(rowFor('alpha')).getByRole('button', { name: /Save row changes/i });
    fireEvent.click(saveButton);

    await waitFor(() => expect(api.api.patchStrategyParams).toHaveBeenCalled());
    const [id, body] = vi.mocked(api.api.patchStrategyParams).mock.calls[0];
    expect(id).toBe('alpha');
    expect(body).toEqual({ qty: '12.5' }); // only dirty field
  });

  it('pressing Enter triggers save', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());
    vi.mocked(api.api.patchStrategyParams).mockResolvedValue({ ok: true } as any);

    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '11' } });
    fireEvent.keyDown(qtyInput, { key: 'Enter' });

    await waitFor(() => expect(api.api.patchStrategyParams).toHaveBeenCalled());
  });

  it('does not show conflict when backend data is unchanged while row is dirty', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '12' } });
    fireEvent.blur(qtyInput);

    // Poll backend with unchanged params; should NOT trigger conflict banner
    await triggerUnchangedUpdate();

    await waitFor(() => {
      expect(screen.queryByText(/Remote update detected/i)).not.toBeInTheDocument();
    });
  });

  it('validation failure focuses first invalid field', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const viewToggle = screen.getByRole('button', { name: /Advanced Params/i });
    fireEvent.click(viewToggle);

    // Set invalid max_age_ms (< 100)
    const maxAgeInput = screen.getAllByDisplayValue('2500')[0] as HTMLInputElement;
    fireEvent.focus(maxAgeInput);
    fireEvent.change(maxAgeInput, { target: { value: '50' } });
    fireEvent.blur(maxAgeInput);

    const saveButton = within(rowFor('alpha')).getByRole('button', { name: /Save row changes/i });
    fireEvent.click(saveButton);

    // After validation error, the invalid input should be focused
    await waitFor(() => {
      const invalidInput = screen.getAllByDisplayValue('50')[0] as HTMLInputElement;
      expect(invalidInput).toHaveAttribute('aria-invalid', 'true');
    });
  });

  it('row revert button restores original value and clears dirty state', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '15' } });
    fireEvent.blur(qtyInput);

    const row = rowFor('alpha');
    const revertButton = within(row).getByRole('button', { name: /Revert row changes/i });
    fireEvent.click(revertButton);

    await waitFor(() => {
      expect(within(row).queryByRole('button', { name: /Revert row changes/i })).toBeNull();
    });
    expect(qtyInput.value).toBe('10');
  });

  it('Revert All button clears every dirty row', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '18' } });
    fireEvent.blur(qtyInput);

    const revertAllButton = screen.getByRole('button', { name: /Revert all dirty rows/i });
    expect(revertAllButton).not.toBeDisabled();
    fireEvent.click(revertAllButton);

    await waitFor(() => expect(revertAllButton).toBeDisabled());
    expect(qtyInput.value).toBe('10');
    await waitFor(() => {
      expect(within(rowFor('alpha')).queryByRole('button', { name: /Save row changes/i })).toBeNull();
    });
  });

  it('shows conflict controls and Keep Mine clears the banner', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '15' } });
    fireEvent.blur(qtyInput);

    await triggerConflictUpdate('999');

    await waitFor(() => expect(screen.getByText(/Remote update detected/i)).toBeInTheDocument());

    const keepMine = screen.getByRole('button', { name: 'Keep Mine' });
    fireEvent.click(keepMine);

    await waitFor(() => {
      expect(screen.queryByText(/Remote update detected/i)).not.toBeInTheDocument();
    });
  });

  it('Use Remote replaces local edits with latest backend values', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '30' } });
    fireEvent.blur(qtyInput);

    await triggerConflictUpdate('77');

    await waitFor(() => expect(screen.getByText(/Remote update detected/i)).toBeInTheDocument());

    const useRemote = screen.getByRole('button', { name: 'Use Remote' });
    fireEvent.click(useRemote);

    await waitFor(() => expect(qtyInput.value).toBe('77'));
    await waitFor(() => {
      expect(within(rowFor('alpha')).queryByRole('button', { name: /Save row changes/i })).toBeNull();
    });
  });

  it('Diff modal shows mine vs remote and can apply remote values', async () => {
    render(<Params />);
    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const qtyInput = screen.getByDisplayValue('10') as HTMLInputElement;
    fireEvent.focus(qtyInput);
    fireEvent.change(qtyInput, { target: { value: '55' } });
    fireEvent.blur(qtyInput);

    await triggerConflictUpdate('80');

    await waitFor(() => expect(screen.getByText(/Remote update detected/i)).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: 'Diff' }));

    const dialog = await screen.findByRole('dialog', { name: /Param diff modal/i });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByText('Mine')).toBeInTheDocument();
    expect(screen.getByText('Remote')).toBeInTheDocument();
    expect(screen.getByText('55')).toBeInTheDocument();
    expect(screen.getByText('80')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Apply Remote Values' }));

    await waitFor(() => expect(screen.queryByRole('dialog', { name: /Param diff modal/i })).not.toBeInTheDocument());
    expect(qtyInput.value).toBe('80');
  });
});
