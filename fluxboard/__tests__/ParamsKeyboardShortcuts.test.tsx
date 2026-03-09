import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import Params from '../Params';
import * as api from '../api';

vi.mock('../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
    updateParams: vi.fn(),
  },
}));

vi.mock('../hooks', () => ({
  usePolling: vi.fn(),
}));

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

describe('Params keyboard shortcuts', () => {
  const mockSchema = {
    params: {
      bot_on: {
        key: 'bot_on',
        label: 'bot_on',
        description: '',
        type: 'select',
        default: '0',
        options: [['0', 'Off'], ['1', 'On']],
      },
      qty: {
        key: 'qty',
        label: 'qty',
        description: '',
        type: 'float',
        default: 1.0,
        min_value: 0.0,
        max_value: 1000.0,
      },
    },
    deprecated: {},
  } as any;

  const mockParams = [
    { strategy_id: 'alpha', running: true, params: { bot_on: '0', qty: '10' } },
    { strategy_id: 'bravo', running: true, params: { bot_on: '0', qty: '20' } },
  ];

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.api.getParamSchema).mockResolvedValue(mockSchema);
    vi.mocked(api.api.getParams).mockResolvedValue(mockParams as any);
    vi.mocked(api.api.updateParams).mockResolvedValue({ success: 1, failed: 0, errors: [] } as any);
    vi.mocked(api.api.patchStrategyParams).mockResolvedValue({ ok: true } as any);
  });

  it('saves a bot_on toggle with Ctrl+Enter after the row switch is focused', async () => {
    render(<Params />);

    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const alphaToggle = screen.getByRole('switch', { name: 'Toggle trading for alpha' });
    fireEvent.focus(alphaToggle);
    fireEvent.click(alphaToggle);

    fireEvent.keyDown(alphaToggle, { key: 'Enter', ctrlKey: true });

    await waitFor(() => expect(api.api.updateParams).toHaveBeenCalledTimes(1));
    expect(api.api.patchStrategyParams).not.toHaveBeenCalled();

    const [updates] = vi.mocked(api.api.updateParams).mock.calls[0];
    expect(updates).toEqual([{ strategy_id: 'alpha', params: { bot_on: '1' } }]);
  });

  it('uses Ctrl+Enter in a param input as Save Selected instead of row save', async () => {
    render(<Params />);

    await waitFor(() => expect(screen.getByText('alpha')).toBeInTheDocument());

    const rows = screen.getByRole('table').querySelectorAll('tbody tr');
    const alphaCell = rows[1].querySelectorAll('td')[0] as HTMLElement;
    const bravoCell = rows[2].querySelectorAll('td')[0] as HTMLElement;

    fireEvent.mouseDown(alphaCell, { button: 0 });
    fireEvent.mouseUp(window);
    fireEvent.mouseDown(bravoCell, { button: 0, ctrlKey: true });
    fireEvent.mouseUp(window);
    expect(screen.getAllByText('2 strategies selected').length).toBeGreaterThan(0);

    const bravoQtyInput = document.querySelector('[data-strategy="bravo"][data-param="qty"]') as HTMLInputElement;

    fireEvent.focus(bravoQtyInput);
    fireEvent.change(bravoQtyInput, { target: { value: '22' } });
    fireEvent.keyDown(bravoQtyInput, { key: 'Enter', ctrlKey: true });

    await waitFor(() => expect(api.api.updateParams).toHaveBeenCalledTimes(1));
    expect(api.api.patchStrategyParams).not.toHaveBeenCalled();

    const [updates] = vi.mocked(api.api.updateParams).mock.calls[0];
    expect(updates).toEqual([
      { strategy_id: 'alpha', params: { qty: '22' } },
      { strategy_id: 'bravo', params: { qty: '22' } },
    ]);
  });
});
