// Alerts panel component for dashboard

import { PanelWrapper } from '../layout/PanelWrapper';
import { lazy, Suspense } from 'react';
const Alerts = lazy(() => import('../../Alerts'));
import { useAlertsStore, selectAlertsFreshnessTs } from '../../stores';
import { STALE_THRESHOLDS } from '../../lib/tokens';

export const AlertsPanel = ({
  title = 'Alerts',
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
  const lastUpdate = useAlertsStore(selectAlertsFreshnessTs);

  return (
    <PanelWrapper
      title={title}
      fullPageUrl="/alerts"
      onRemove={onRemove}
      onCollapsedChange={onCollapsedChange}
      lastUpdate={lastUpdate}
      staleThresholdMs={STALE_THRESHOLDS.NORMAL}
      fullWidth={fullWidth}
      collapsed={collapsed}
    >
      <Suspense fallback={<div /> }>
        <Alerts dense showHeader={false} />
      </Suspense>
    </PanelWrapper>
  );
};

(AlertsPanel as any).displayName = 'Alerts';
(AlertsPanel as any).defaultSize = { w: 12, h: 4, minW: 4, maxW: 12 };
