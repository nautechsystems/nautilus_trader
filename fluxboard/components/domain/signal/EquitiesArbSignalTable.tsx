import { useMemo } from 'react';

import { deriveStrategyProfile } from '@/config/paramsProfiles';
import type { SignalStrategy } from '@/types';

import MakerV4SignalTable from './MakerV4SignalTable';

function resolveEquitiesSplitFamily(
  row: Pick<SignalStrategy, 'strategy_family' | 'meta' | 'params' | 'hot_params'>,
): 'equities_maker' | 'equities_taker' | null {
  const strategyFamily = String(row.strategy_family ?? '').trim().toLowerCase();
  if (strategyFamily === 'equities_maker' || strategyFamily === 'equities_taker') {
    return strategyFamily;
  }

  const metaFamily = String(row.meta?.strategy_family ?? '').trim().toLowerCase();
  if (metaFamily === 'equities_maker' || metaFamily === 'equities_taker') {
    return metaFamily;
  }

  const className = String(row.meta?.class ?? '').trim().toLowerCase();
  if (className === 'equities_maker' || className === 'equities_taker') {
    return className;
  }

  const profile = deriveStrategyProfile(row);
  return profile === 'equities_maker' || profile === 'equities_taker' ? profile : null;
}

export default function EquitiesArbSignalTable({
  rows,
  strategies,
  loading = false,
  nowProvider,
}: {
  rows?: SignalStrategy[];
  strategies?: SignalStrategy[];
  loading?: boolean;
  nowProvider?: () => number;
}) {
  const sourceRows = rows ?? strategies ?? [];
  const activeRows = useMemo(
    () => sourceRows.filter((row) => row.equities_arb && resolveEquitiesSplitFamily(row) != null),
    [sourceRows],
  );

  return (
    <MakerV4SignalTable
      rows={activeRows}
      loading={loading}
      nowProvider={nowProvider}
      payloadKey="equities_arb"
      showVariantColumn
      tableTestId="equities-arb-signal-table"
      emptyMessage="No equities signals"
    />
  );
}
