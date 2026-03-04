import { describe, it, expect } from 'vitest';
import { SCANNERS_COLUMN_SEQUENCE } from '@/components/domain/scanners/ScannersTable';

describe('Scanners column headers', () => {
  it('uses the compact header sequence', () => {
    expect([...SCANNERS_COLUMN_SEQUENCE]).toEqual([
      'Pool',
      'DEX',
      'Chain',
      'Best Edge',
      'Vol 24h',
      'TVL',
      'Leg A',
      'Leg B',
      'DEX Fee',
      'CEX Fee',
      'B->A Edge',
      'A->B Edge',
      'Marginable',
      'Last Update',
      'Local Time',
    ]);
  });
});
