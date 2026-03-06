import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  }
}));

// Lazy import after mocks
import { api } from '../api';

const okJson = (body: any, status = 200, headers?: Record<string, string>) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', ...(headers || {}) }
  });

describe('PnL API envelope handling', () => {
  const origFetch = global.fetch;

  beforeEach(() => {
    // @ts-ignore
    global.fetch = vi.fn();
  });

  afterEach(() => {
    global.fetch = origFetch as any;
    vi.restoreAllMocks();
  });

  it('runPnLReport unwraps fluxapi envelope on success', async () => {
    (global.fetch as any).mockResolvedValueOnce(
      okJson({ ok: true, data: { summary: { count: 0 }, groups: [], unhedged: {} }, error: null })
    );

    const result = await api.runPnLReport({ minutes: 5, last: null, base: null, dex_fee_bps: 2, cex_fee_bps: 5, dex: 'auto', cex: 'bybit' });
    expect(result.summary.count).toBe(0);
    expect(Array.isArray(result.groups)).toBe(true);
  });

  it('runPnLReport throws with fluxapi error message', async () => {
    (global.fetch as any).mockResolvedValueOnce(
      okJson({ ok: false, data: null, error: 'invalid_request' })
    );
    await expect(api.runPnLReport({ minutes: 5, last: null, base: null, dex_fee_bps: 2, cex_fee_bps: 5, dex: 'auto', cex: 'bybit' }))
      .rejects.toThrow('invalid_request');
  });

  it('getAvailableSymbols unwraps envelope and prefers bases', async () => {
    (global.fetch as any).mockResolvedValueOnce(
      okJson({ ok: true, data: { bases: ['PLUME', 'ETH'], symbols: ['PLUME/USDT', 'ETH/USDC'], count: 2 }, error: null })
    );
    const bases = await api.getAvailableSymbols();
    expect(bases).toEqual(['PLUME', 'ETH']);
  });

  it('downloadPnLCSV surfaces failure error', async () => {
    (global.fetch as any).mockResolvedValueOnce({
      ok: false,
      status: 400,
      statusText: 'Bad Request',
      headers: new Headers({ 'Content-Type': 'application/json' }),
      json: async () => ({ ok: false, data: null, error: 'rate_limited' }),
    });
    await expect(api.downloadPnLCSV({ minutes: 5, last: null, base: null, dex_fee_bps: 2, cex_fee_bps: 5, dex: 'auto', cex: 'bybit' }))
      .rejects.toThrow(/CSV download failed/);
  });

  it('downloadPnLCSV handles success path and triggers download', async () => {
    Object.defineProperty(window.URL, 'createObjectURL', { value: vi.fn(() => 'blob:mock'), configurable: true });
    Object.defineProperty(window.URL, 'revokeObjectURL', { value: vi.fn(), configurable: true });
    const appendSpy = vi.spyOn(document.body, 'appendChild').mockImplementation(() => (null as any));
    const removeSpy = vi.spyOn(document.body, 'removeChild').mockImplementation(() => (null as any));
    const createSpy = vi.spyOn(document, 'createElement').mockImplementation(() => ({
      href: '',
      download: '',
      click: vi.fn(),
    } as any));

    (global.fetch as any).mockResolvedValueOnce({
      ok: true,
      status: 200,
      statusText: 'OK',
      headers: new Headers({ 'Content-Disposition': 'attachment; filename="pnl.zip"' }),
      blob: async () => new Blob([new Uint8Array([1,2,3])], { type: 'application/zip' }),
    });
    await api.downloadPnLCSV({ minutes: 5, last: null, base: null, dex_fee_bps: 2, cex_fee_bps: 5, dex: 'auto', cex: 'bybit' });
    expect((window.URL as any).createObjectURL).toHaveBeenCalled();
    appendSpy.mockRestore();
    removeSpy.mockRestore();
    createSpy.mockRestore();
  });
});
