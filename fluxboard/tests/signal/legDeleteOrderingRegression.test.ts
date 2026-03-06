import { describe, expect, it } from 'vitest';

import { getOrderedLegKeys, mergeSignalLegMaps } from '@/utils/signalLegs';

describe('signal leg delete + ordering fallback regression', () => {
  it('drops deleted keys and falls back to lexical ordering when legs_order is cleared', () => {
    const previousLegs: any = {
      contract_z: { coin: 'BTC-Z' },
      contract_a: { coin: 'BTC-A' },
      contract_stale_null: null,
    };

    const merged = mergeSignalLegMaps(previousLegs, {
      contract_z: null,
    } as any);

    // Delete semantics: key removed entirely
    expect('contract_z' in merged).toBe(false);
    // Legacy/stale null entries must not participate in ordering.
    expect(getOrderedLegKeys(merged, ['contract_stale_null', 'contract_a'])).toEqual(['contract_a']);

    // legs_order explicit clear => lexical fallback over remaining non-null keys.
    expect(getOrderedLegKeys(merged, null)).toEqual(['contract_a']);
  });
});
