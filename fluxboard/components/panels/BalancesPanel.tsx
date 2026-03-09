// Balances panel component for dashboard

import { PanelWrapper } from '../layout/PanelWrapper';
import { lazy, Suspense } from 'react';
import { useBalancesStore, selectBalancesFreshnessTs } from '../../stores';
import { STALE_THRESHOLDS } from '../../lib/tokens';

const Balances = lazy(() => import('../../Balances'));

type BalancesPanelProps = {
  title?: string;
  onRemove?: () => void;
  onCollapsedChange?: (collapsed: boolean) => void;
  fullWidth?: boolean;
  collapsed?: boolean;
};

type PanelSize = {
  w: number;
  h: number;
  minW?: number;
  maxW?: number;
  minH?: number;
  maxH?: number;
};

const BalancesPanelComponent = ({
  title = 'Balances',
  onRemove,
  onCollapsedChange,
  fullWidth,
  collapsed
}: BalancesPanelProps) => {
  // Optimized: only re-renders when data freshness changes
  const lastUpdate = useBalancesStore(selectBalancesFreshnessTs);

  return (
    <PanelWrapper
      title={title}
      fullPageUrl="/balances"
      onRemove={onRemove}
      onCollapsedChange={onCollapsedChange}
      lastUpdate={lastUpdate}
      staleThresholdMs={STALE_THRESHOLDS.SLOW}
      fullWidth={fullWidth}
      collapsed={collapsed}
    >
      <Suspense fallback={<div />}>
        <Balances dense showHeader={false} />
      </Suspense>
    </PanelWrapper>
  );
};

export const BalancesPanel = Object.assign(BalancesPanelComponent, {
  displayName: 'Balances',
  defaultSize: { w: 4, h: 4, minW: 3, maxW: 12 } satisfies PanelSize,
});
