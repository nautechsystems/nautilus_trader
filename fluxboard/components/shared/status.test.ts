import { describe, expect, it, vi } from 'vitest';
import { statusFromAge, statusFromFxPair, statusFromMark } from './status';
import type { FxPair } from '@/types';
import { deriveFxStatus } from '@/utils';

vi.mock('@/utils', () => ({
  deriveFxStatus: vi.fn(),
}));

const mockedFx = vi.mocked(deriveFxStatus);

describe('status helpers', () => {
  beforeEach(() => {
    mockedFx.mockReset();
  });

  it('maps FX pair status', () => {
    mockedFx.mockReturnValue('yellow' as any);
    const descriptor = statusFromFxPair({ fallback: true } as FxPair);
    expect(descriptor.status).toBe('warning');
    expect(descriptor.label).toBe('FALLBACK');
  });

  it('derives age status thresholds', () => {
    const fresh = statusFromAge(Date.now());
    expect(fresh.status).toBe('ok');
    const stale = statusFromAge(Date.now() - 40 * 60 * 1000);
    expect(stale.status).toBe('critical');
  });

  it('handles mark deviations', () => {
    expect(statusFromMark(1, true).status).toBe('ok');
    expect(statusFromMark(1.1, true).status).toBe('critical');
    expect(statusFromMark(null, true).status).toBe('muted');
  });
});
