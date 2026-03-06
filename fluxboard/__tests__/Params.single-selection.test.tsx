import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';

import Params from '../Params';
import { useParamsStore } from '../stores';
import * as apiModule from '../api';

vi.mock('../api', () => ({
  api: {
    getParamSchema: vi.fn(),
    getParams: vi.fn(),
    patchStrategyParams: vi.fn(),
    updateParams: vi.fn(),
  },
}));

vi.mock('../hooks/index', () => ({
  usePolling: vi.fn(),
}));

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

describe('Params single-strategy auto selection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useParamsStore.getState().clearSelection();

    vi.mocked(apiModule.api.getParamSchema).mockResolvedValue({
      params: {
        bot_on: {
          key: 'bot_on',
          label: 'Bot On',
          description: 'Enable trading',
          type: 'select',
          default: '0',
          options: [['0', 'Off'], ['1', 'On']],
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(apiModule.api.getParams).mockResolvedValue([
      {
        strategy_id: 'makerv3',
        running: true,
        params: { bot_on: '1' },
      },
    ] as any);
  });

  it('selects the only strategy so toolbar/actions are populated', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('makerv3')).toBeInTheDocument();
    });

    expect(screen.getByText('1 selected')).toBeInTheDocument();
  });

  it('re-selects the only visible strategy after selection is cleared', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('1 selected')).toBeInTheDocument();
    });

    act(() => {
      useParamsStore.getState().clearSelection();
    });

    await waitFor(() => {
      expect(screen.getByText('1 selected')).toBeInTheDocument();
    });
  });
});
