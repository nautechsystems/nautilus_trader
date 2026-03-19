import type { SignalStrategy } from '@/types';

import { ArbSignalTable } from './EquitiesArbSignalTable';

type MakerV4SignalTableProps = {
  rows?: SignalStrategy[];
  strategies?: SignalStrategy[];
  loading?: boolean;
  nowProvider?: () => number;
  showVariantColumn?: boolean;
  tableTestId?: string;
  emptyMessage?: string;
};

export default function MakerV4SignalTable({
  rows,
  strategies,
  loading = false,
  nowProvider = () => Date.now(),
  showVariantColumn = false,
  tableTestId = 'maker-v4-signal-table',
  emptyMessage = 'No Maker V4 strategies found',
}: MakerV4SignalTableProps) {
  return (
    <ArbSignalTable
      rows={rows}
      strategies={strategies}
      loading={loading}
      nowProvider={nowProvider}
      payloadKey="maker_v4"
      showVariantColumn={showVariantColumn}
      tableTestId={tableTestId}
      emptyMessage={emptyMessage}
    />
  );
}
