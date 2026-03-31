/**
 * Unit tests for API client.
 *
 * Tests all API methods with mocked fetch including:
 * - Successful responses
 * - Error handling (4xx, 5xx, network errors)
 * - Response parsing
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { toast } from 'sonner';
import { api } from '../api';
import { APIError } from '../apiClient';
import type { ParamUpdate } from '../types';

process.on('unhandledRejection', (err) => {
  if (err instanceof APIError) {
    return;
  }
  if (err instanceof Error) {
    const message = err.message || '';
    if (
      message.includes('Network error') ||
      message.includes('Not Found') ||
      message.includes('Bad Request') ||
      message.includes('Internal Server Error') ||
      message.includes('Failed to fetch') ||
      message.includes('Timeout') ||
      message.includes('Invalid JSON')
    ) {
      return;
    }
  }
  throw err;
});

// Mock fetch globally
const mockFetch = vi.fn();
global.fetch = mockFetch;

// Mock toast
vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn()
  }
}));

describe('API Client - Param Methods', () => {
  beforeEach(() => {
    (window.location as any).pathname = '/';
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  describe('getParamSchema', () => {
    it('fetches parameter schema successfully', async () => {
      const mockSchema = {
        params: {
          bot_on: { key: 'bot_on', type: 'select', default: '0' },
          max_cancels_per_side_per_cycle: {
            key: 'max_cancels_per_side_per_cycle',
            type: 'integer',
            default: 1,
            advanced: true,
          },
        },
        deprecated: {}
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: mockSchema, error: null })
      });

      const result = await api.getParamSchema();

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/param-schema$/),
        expect.objectContaining({ signal: expect.any(AbortSignal) })
      );
      // API layer normalizes param defs (adds label/description/options/etc).
      expect(result.deprecated).toEqual({});
      expect(result.params.bot_on).toMatchObject({
        key: 'bot_on',
        type: 'select',
        default: '0',
        label: 'Trading',
        description: 'Trading gate. Controls whether the strategy may place new orders. Independent of runner state. 1 = Enabled, 0 = Paused.',
        deprecated: false,
        replacement: null,
        step: null,
        unit: null,
      });
      expect(result.params.bot_on.options).toEqual([
        ['0', 'Paused (0)'],
        ['1', 'Enabled (1)'],
      ]);
      expect(result.params.max_cancels_per_side_per_cycle.advanced).toBe(true);
    });

    it('throws error on 500 response', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
        statusText: 'Internal Server Error'
      });

      await expect(api.getParamSchema()).rejects.toThrow('500 Internal Server Error');
    });

    it('throws error on network failure', async () => {
      mockFetch.mockRejectedValueOnce(new Error('Network error'));

      await expect(api.getParamSchema()).rejects.toThrow('Network error');
    });
  });

  describe('getParams', () => {
    it('fetches all strategy params successfully', async () => {
      const mockParams = [
        { strategy_id: 'strat1', running: true, params: { bot_on: '1', qty: '10' } },
        { strategy_id: 'strat2', running: false, params: { bot_on: '0', qty: '20' } }
      ];

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: mockParams, error: null })
      });

      const result = await api.getParams();

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/params$/),
        expect.objectContaining({ signal: expect.any(AbortSignal) })
      );
      expect(result).toEqual([
        { strategy_id: 'strat1', params: { bot_on: '1', qty: '10' }, running: true },
        { strategy_id: 'strat2', params: { bot_on: '0', qty: '20' }, running: false },
      ]);
      expect(result.length).toBe(2);
    });

    it('does not infer running from bot_on when params payload omits it', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          ok: true,
          data: [{ strategy_id: 'strat1', params: { bot_on: '1', qty: '10' } }],
          error: null,
        }),
      });

      const result = await api.getParams();

      expect(result).toEqual([
        { strategy_id: 'strat1', params: { bot_on: '1', qty: '10' }, running: null },
      ]);
    });

    it('handles empty array response', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: [], error: null })
      });

      const result = await api.getParams();
      expect(result).toEqual([]);
    });

    it('throws error on 404', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 404,
        statusText: 'Not Found'
      });

      await expect(api.getParams()).rejects.toThrow('404 Not Found');
    });
  });

  describe('patchStrategyParams', () => {
    it('updates strategy params with PATCH', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: {}, error: null })
      });

      const params = { qty: '15.0', bot_on: '1' };
      const result = await api.patchStrategyParams('strat1', params, 'fluxboard');
      const updates = [{ strategy_id: 'strat1', params }];

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/params$/),
        expect.objectContaining({
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ updates, source: 'fluxboard' })
        })
      );
      expect(result).toEqual({ ok: true });
    });

    it('uses default source if not provided', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: {}, error: null })
      });

      const params = { qty: '10' };
      await api.patchStrategyParams('strat1', params);

      const call = mockFetch.mock.calls[0];
      const body = JSON.parse(call[1].body);
      expect(body.source).toBe('fluxboard');
    });

    it('throws error on validation failure (400)', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        statusText: 'Bad Request'
      });

      await expect(
        api.patchStrategyParams('strat1', { qty: '-10' })
      ).rejects.toThrow('400 Bad Request');
    });

    it('preserves backend validation message on 400 responses', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        statusText: 'Bad Request',
        json: async () => ({
          ok: false,
          error: {
            code: 'invalid_params_update',
            message: '`bid_edge1` must be <= 1000.0',
          },
        }),
      });

      await expect(
        api.patchStrategyParams('strat1', { bid_edge1: '1001' })
      ).rejects.toThrow('`bid_edge1` must be <= 1000.0');
    });
  });

  describe('updateParams', () => {
    it('bulk updates multiple strategies', async () => {
      const mockResult = {
        success: 2,
        failed: 0,
        errors: []
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: mockResult, error: null })
      });

      const updates = [
        { strategy_id: 'strat1', params: { bot_on: '1' } },
        { strategy_id: 'strat2', params: { bot_on: '0' } }
      ];

      const result = await api.updateParams(updates, 'fluxboard');

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/params$/),
        expect.objectContaining({
          method: 'PATCH',
          body: JSON.stringify({ updates, source: 'fluxboard' })
        })
      );
      expect(result.success).toBe(2);
      expect(result.failed).toBe(0);
    });

    it('handles partial failures', async () => {
      const mockResult = {
        success: 1,
        failed: 1,
        errors: [{ strategy_id: 'strat2', error: 'Validation failed' }]
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: mockResult, error: null })
      });

      const updates: ParamUpdate[] = [
        { strategy_id: 'strat1', params: { bot_on: '1' } },
        { strategy_id: 'strat2', params: { qty: '-10' } }
      ];

      const result = await api.updateParams(updates);

      expect(result.success).toBe(1);
      expect(result.failed).toBe(1);
      expect(result.errors.length).toBe(1);
    });

    it('returns 400 on validation errors', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        statusText: 'Bad Request'
      });

      const updates = [
        { strategy_id: 'strat1', params: { qty: 'invalid' } }
      ];

      await expect(api.updateParams(updates)).rejects.toThrow('400 Bad Request');
    });

    it('preserves backend validation message for bulk 400 responses', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        statusText: 'Bad Request',
        json: async () => ({
          ok: false,
          error: {
            code: 'invalid_params_update',
            message: 'strat1: `bid_edge1` must be <= 1000.0',
          },
        }),
      });

      const updates = [
        { strategy_id: 'strat1', params: { bid_edge1: '1001' } }
      ];

      await expect(api.updateParams(updates)).rejects.toThrow(
        'strat1: `bid_edge1` must be <= 1000.0',
      );
    });

    it('uses default source if not provided', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: { success: 1, failed: 0, errors: [] }, error: null })
      });

      const updates = [{ strategy_id: 'strat1', params: { bot_on: '1' } }];
      await api.updateParams(updates);

      const call = mockFetch.mock.calls[0];
      const body = JSON.parse(call[1].body);
      expect(body.source).toBe('fluxboard');
    });
  });

  describe('getStrategyConfig', () => {
    it('fetches strategy config successfully', async () => {
      const mockConfig = {
        strategies_ini: '[strat1]\nqty = 10',
        relations_ini: '[rel1]\nstrategy = strat1',
        catalog_excerpts: '# Catalog'
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockConfig
      });

      const result = await api.getStrategyConfig('strat1');

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/strategies\/strat1\/config(-files)?$/),
        expect.objectContaining({ signal: expect.any(AbortSignal) })
      );
      expect(result.strategies_ini).toContain('[strat1]');
      expect(result.relations_ini).toContain('rel1');
    });

    it('handles missing strategy gracefully', async () => {
      const mockConfig = {
        strategies_ini: '# Strategy not found',
        relations_ini: '# No relations found',
        catalog_excerpts: '# Catalog'
      };

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => mockConfig
      });

      const result = await api.getStrategyConfig('unknown_strat');

      expect(result.strategies_ini).toContain('not found');
    });

    it('throws error on 500 response', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
        statusText: 'Internal Server Error'
      });

      await expect(api.getStrategyConfig('strat1')).rejects.toThrow('500');
    });
  });

  describe('TokenMM profile query propagation', () => {
    it('adds profile query for balances on tokenmm paths', async () => {
      (window.location as any).pathname = '/tokenmm/balances';
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          data: {
            rows: [],
            total: 0,
            totals: { mv_raw: 0, mv_display: '$0.00' },
            generated_at: new Date().toISOString(),
            view: 'parents_only',
            risk_groups: [],
          },
        }),
      });

      await api.getBalances();

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/balances\?profile=tokenmm$/),
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      );
    });

    it('normalizes flat balances rows into parent/children rows for tokenmm payloads', async () => {
      (window.location as any).pathname = '/tokenmm/balances';
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          ok: true,
          data: {
            rows: [
              { exchange: 'bybit', asset: 'PLUME', total: '100', ts_ms: 1700000000000 },
              { exchange: 'binance', asset: 'PLUME', total: '50', ts_ms: 1700000001000 },
            ],
            count: 2,
            server_ts_ms: 1700000002000,
          },
          error: null,
        }),
      });

      const result = await api.getBalances();

      expect(result.rows).toHaveLength(1);
      expect(result.rows[0].canonical).toBe('PLUME');
      expect(result.rows[0].children).toHaveLength(2);
      expect(result.rows[0].children[0].parent_id).toBe('PLUME_LOGICAL');
      expect(result.total).toBe(2);
      expect(result.view).toBe('parents_only');
      expect(result.generated_at).toBeTruthy();
      expect(result.totals.mv_raw).toBe(0);
    });

    it('adds profile query for alerts on tokenmm paths', async () => {
      (window.location as any).pathname = '/tokenmm/alerts';
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          ok: true,
          data: {
            rows: [],
            total: 0,
            capabilities: {
              feed_mode: 'active',
              clear_mode: 'history_only',
            },
          },
          error: null,
        }),
      });

      const result = await api.getAlerts();

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/alerts\?profile=tokenmm$/),
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      );
      expect((result as typeof result & {
        capabilities?: { feed_mode?: string; clear_mode?: string };
      }).capabilities).toEqual({
        feed_mode: 'active',
        clear_mode: 'history_only',
      });
    });

    it('normalizes clearAlerts response and includes tokenmm profile query', async () => {
      (window.location as any).pathname = '/tokenmm/alerts';
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          ok: true,
          data: {
            deleted: 2,
            remaining: 1,
            strategy_id: 'strategy_01',
            server_ts_ms: 1234,
            capabilities: {
              feed_mode: 'active',
              clear_mode: 'history_only',
            },
          },
          error: null,
        }),
      });

      const result = await api.clearAlerts();

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringMatching(/\/api\/v1\/alerts\?profile=tokenmm$/),
        expect.objectContaining({
          method: 'DELETE',
          headers: { 'Content-Type': 'application/json' },
        }),
      );
      expect(result).toEqual({
        success: true,
        deleted: 2,
        remaining: 1,
        capabilities: {
          feed_mode: 'active',
          clear_mode: 'history_only',
        },
      });
      expect(toast.success).toHaveBeenCalledWith('Alert history cleared');
    });
  });

  describe('Error handling edge cases', () => {
    it('handles malformed JSON response', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => {
          throw new Error('Invalid JSON');
        }
      });

      await expect(api.getParams()).rejects.toThrow('Invalid JSON');
    });

    it('handles timeout', async () => {
      mockFetch.mockImplementationOnce(() =>
        new Promise((_, reject) =>
          setTimeout(() => reject(new Error('Timeout')), 100)
        )
      );

      await expect(api.getParams()).rejects.toThrow('Timeout');
    });

    it('handles CORS errors', async () => {
      mockFetch.mockRejectedValueOnce(new TypeError('Failed to fetch'));

      await expect(api.getParams()).rejects.toThrow('Failed to fetch');
    });
  });
});

describe('API Client - Existing Methods', () => {
  beforeEach(() => {
    (window.location as any).pathname = '/';
    mockFetch.mockReset();
  });

  describe('updateStrategyParams (PATCH)', () => {
    it('updates strategy params successfully', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: {}, error: null })
      });

      const params = { bot_on: '1', qty: '10' };
      const result = await api.updateStrategyParams('strat1', params);
      const updates = [{ strategy_id: 'strat1', params }];

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/v1/params'),
        expect.objectContaining({
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ updates, source: 'fluxboard' })
        })
      );
      expect(result).toEqual({ ok: true });
    });
  });

  describe('getStrategies', () => {
    it('fetches strategy list', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: { strategies: [{ id: 'strat1' }, { id: 'strat2' }] }, error: null })
      });

      const result = await api.getStrategies();

      expect(result).toEqual(['strat1', 'strat2']);
    });

    it('falls back to legacy rows key when strategies is missing', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: { rows: [{ id: 'strat1' }] }, error: null })
      });

      const result = await api.getStrategies();

      expect(result).toEqual(['strat1']);
    });
  });

  describe('getStrategiesWithStatus', () => {
    it('fetches strategies with status', async () => {
      const mockStrategies = [
        { id: 'strat1', status: { running: true } }
      ];

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: { strategies: mockStrategies, count: mockStrategies.length }, error: null })
      });

      const result = await api.getStrategiesWithStatus();

      expect(result).toEqual(mockStrategies);
    });
  });

  describe('getStrategyParams', () => {
    it('fetches and stringifies strategy params from params payload key', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: { strategy_id: 'strat1', params: { bot_on: true, qty: 10 } }, error: null }),
      });

      const result = await api.getStrategyParams('strat1');

      expect(result).toEqual({ bot_on: 'true', qty: '10' });
    });

    it('falls back to legacy parameters key when params is missing', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ ok: true, data: { strategy_id: 'strat1', parameters: { qty: 5 } }, error: null }),
      });

      const result = await api.getStrategyParams('strat1');

      expect(result).toEqual({ qty: '5' });
    });
  });
});
