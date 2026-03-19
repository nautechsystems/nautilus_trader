import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';

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

describe('Params family short headers', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (window.location as any).pathname = '/equities/params';
    useParamsStore.getState().clearSelection();
    useParamsStore.getState().setActiveProfile('equities_maker' as any);

    vi.mocked(apiModule.api.getParamSchema).mockResolvedValue({
      params: {
        hedge_style: {
          key: 'hedge_style',
          label: 'hedge_style',
          description: 'hedge mode',
          type: 'select',
          default: 'ioc_through_mid',
          options: [['ioc_through_mid', 'IOC Through Mid']],
        },
        assumed_hedge_fee_bps: {
          key: 'assumed_hedge_fee_bps',
          label: 'assumed_hedge_fee_bps',
          description: 'assumed hedge fee',
          type: 'float',
          default: 1,
        },
      },
      deprecated: {},
    } as any);
    vi.mocked(apiModule.api.getParams).mockResolvedValue([
      {
        strategy_id: 'aapl_tradexyz_maker',
        running: true,
        meta: {
          class: 'equities_maker',
          param_set: 'equities_maker',
          strategy_family: 'equities_maker',
        },
        hot_params: ['hedge_style', 'assumed_hedge_fee_bps'],
        params: {
          hedge_style: 'ioc_through_mid',
          assumed_hedge_fee_bps: '1',
        },
      },
    ] as any);
  });

  it('uses key-label headers for the split equities maker profile', async () => {
    render(<Params />);

    await waitFor(() => {
      expect(screen.getByText('aapl_tradexyz_maker')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(apiModule.api.getParamSchema).toHaveBeenCalledWith({
        preferKeyLabel: true,
        strategyId: 'aapl_tradexyz_maker',
      });
    });

    expect(screen.getByRole('button', { name: 'Sort by hedge_style' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Sort by assumed_hedge_fee_bps' }),
    ).toBeInTheDocument();
  });
});
