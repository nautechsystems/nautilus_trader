import { render, waitFor, screen } from '@testing-library/react';
import { describe, it, vi, beforeEach } from 'vitest';
import Scanner from '@/Scanner';
import * as apiModule from '@/api';

vi.mock('@/api');

describe('ScannersTable minimal', () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it('keeps existing default (pcs_bnb_usdt) even when equity_hl_futu exists, and renders scanner select', async () => {
    vi.mocked(apiModule.api.getScannersRegistry).mockResolvedValue({
      scanners: [
        { scanner_id: 'equity_hl_futu', dex_name: 'hyperliquid', chain: 'equity', health: { is_healthy: true, age_ms: 1000 } },
        { scanner_id: 'pcs_bnb_usdt', dex_name: 'pancakeswap_v3', chain: 'bnb', health: { is_healthy: true, age_ms: 2000 } },
      ],
      total: 2,
    } as any);

    vi.mocked(apiModule.api.getScannerPricingSnapshots).mockResolvedValue({
      snapshots: [],
      total: 0,
      pageInfo: { next_cursor: null, has_more: false, limit: 200, sort_by: 'last_update_ts', sort_dir: 'desc' },
    } as any);

    render(<Scanner />);

    await waitFor(() => {
      expect(apiModule.api.getScannersRegistry).toHaveBeenCalled();
    });

    const select = await screen.findByLabelText('Scanner');
    await waitFor(() => {
      expect((select as HTMLSelectElement).value).toBe('pcs_bnb_usdt');
    });
  });
});
