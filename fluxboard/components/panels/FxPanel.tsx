// FX panel component for dashboard

import { PanelWrapper } from '../layout/PanelWrapper';
import { lazy, Suspense } from 'react';
const Fx = lazy(() => import('../../Fx'));
import { useFxStore, selectFxLastFetch } from '../../stores';
import { STALE_THRESHOLDS } from '../../lib/tokens';

export const FxPanel = ({
  title = 'FX',
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
  const lastUpdate = useFxStore(selectFxLastFetch);

  return (
    <PanelWrapper
      title={title}
      fullPageUrl="/fx"
      onRemove={onRemove}
      onCollapsedChange={onCollapsedChange}
      lastUpdate={lastUpdate}
      staleThresholdMs={STALE_THRESHOLDS.FAST}
      fullWidth={fullWidth}
      collapsed={collapsed}
    >
      <Suspense fallback={<div /> }>
        <Fx dense showHeader={false} />
      </Suspense>
    </PanelWrapper>
  );
};

(FxPanel as any).displayName = 'FX';
(FxPanel as any).defaultSize = { w: 12, h: 4 };
