import type { SignalStrategy } from '@/types';

import MakerV4SignalTable from './MakerV4SignalTable';

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
  return (
    <MakerV4SignalTable
      rows={rows}
      strategies={strategies}
      loading={loading}
      nowProvider={nowProvider}
      payloadKey="equities_arb"
      showVariantColumn
      tableTestId="equities-arb-signal-table"
      emptyMessage="No equities signals"
    />
  );
}
