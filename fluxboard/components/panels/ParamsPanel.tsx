// Params panel component for dashboard

import { PanelWrapper } from '../layout/PanelWrapper';
import { lazy, Suspense } from 'react';
const Params = lazy(() => import('../../Params'));
import { useParamsStore } from '../../stores';
import { STALE_THRESHOLDS } from '../../lib/tokens';

export const ParamsPanel = ({
  title = 'Params',
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
  const lastUpdate = useParamsStore((state) => state.lastUpdate);

  return (
    <PanelWrapper
      title={title}
      fullPageUrl="/params"
      onRemove={onRemove}
      onCollapsedChange={onCollapsedChange}
      lastUpdate={lastUpdate}
      staleThresholdMs={STALE_THRESHOLDS.MANUAL}
      fullWidth={fullWidth}
      collapsed={collapsed}
    >
      <Suspense fallback={<div /> }>
        <Params showHeader={false} />
      </Suspense>
    </PanelWrapper>
  );
};

(ParamsPanel as any).displayName = 'Params';
(ParamsPanel as any).defaultSize = { w: 12, h: 4, minW: 6, maxW: 12 };
