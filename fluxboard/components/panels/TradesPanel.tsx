// Trades panel component for dashboard

import { PanelWrapper } from '../layout/PanelWrapper';
import { lazy, Suspense } from 'react';
const Trades = lazy(() => import('../../Trades'));
import { useTradesStore, selectTradesFreshnessTs } from '../../stores';
import { STALE_THRESHOLDS } from '../../lib/tokens';

export const TradesPanel = ({
  title = 'Trades',
  onRemove,
  onCollapsedChange,
  fullWidth,
  collapsed
}: {
  title?: string;
  onRemove?: () => void;
  onCollapsedChange?: (collapsed: boolean) => void;
  fullWidth?: boolean;
  collapsed?: boolean;
}) => {
  // Optimized: only re-renders when data freshness changes
  const lastUpdate = useTradesStore(selectTradesFreshnessTs);

  return (
    <PanelWrapper
      title={title}
      fullPageUrl="/trades"
      onRemove={onRemove}
      onCollapsedChange={onCollapsedChange}
      lastUpdate={lastUpdate}
      staleThresholdMs={STALE_THRESHOLDS.REALTIME}
      fullWidth={fullWidth}
      collapsed={collapsed}
    >
      <Suspense fallback={<div /> }>
        <Trades dense showHeader={false} />
      </Suspense>
    </PanelWrapper>
  );
};

(TradesPanel as any).displayName = 'Trades';
(TradesPanel as any).defaultSize = { w: 12, h: 5, minW: 6, maxW: 12, minH: 4 };
