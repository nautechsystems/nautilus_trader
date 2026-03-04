import { beforeEach, describe, expect, it, vi } from 'vitest';

const fetchJSONMock = vi.hoisted(() => vi.fn());

vi.mock('./apiClient', () => {
  class MockAPIClient {
    fetchJSON(path: string, init?: RequestInit) {
      return fetchJSONMock(path, init);
    }
  }
  return { APIClient: MockAPIClient };
});

import { api } from './api';

function setPathname(pathname: string) {
  (window.location as unknown as { pathname?: string }).pathname = pathname;
}

describe('api.getTrades', () => {
  beforeEach(() => {
    fetchJSONMock.mockReset();
    setPathname('/');
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        rows: [],
        total: 0,
        limit: 50,
        offset: 0,
        has_more: true,
        next_offset: 50,
        next_cursor: 'cursor-token',
      },
    });
  });

  it('sends FluxAPI pagination params (limit/offset) instead of legacy page fields', async () => {
    await api.getTrades(3, 25, { sort: 'ts_desc' });

    expect(fetchJSONMock).toHaveBeenCalledTimes(1);
    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);

    expect(params.get('limit')).toBe('25');
    expect(params.get('offset')).toBe('50');
    expect(params.has('page')).toBe(false);
    expect(params.has('page_size')).toBe(false);
  });

  it('returns pagination metadata when provided by FluxAPI', async () => {
    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });

    expect(result.has_more).toBe(true);
    expect(result.next_offset).toBe(50);
    expect(result.next_cursor).toBe('cursor-token');
  });

  it('passes cursor param when present', async () => {
    await api.getTrades(1, 50, { cursor: 'abc', sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('cursor=abc');
    expect(path).toContain('offset=0');
  });

  it('appends tokenmm profile for tokenmm routes', async () => {
    setPathname('/tokenmm/trades');

    await api.getTrades(1, 25, { sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);
    expect(params.get('profile')).toBe('tokenmm');
  });
});

describe('profile-scoped read APIs', () => {
  beforeEach(() => {
    fetchJSONMock.mockReset();
    setPathname('/');
  });

  it('appends profile to signals request on equities routes', async () => {
    setPathname('/equities/signal');
    fetchJSONMock.mockResolvedValue({ ok: true, data: { strategies: [] } });

    await api.getSignals();

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/signals?');
    expect(path).toContain('profile=equities');
  });

  it('appends profile to params request on tokenmm routes', async () => {
    setPathname('/tokenmm/params');
    fetchJSONMock.mockResolvedValue({ ok: true, data: [] });

    await api.getParams();

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/params?');
    expect(path).toContain('profile=tokenmm');
  });
});
