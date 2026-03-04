import { lazy, Suspense } from 'react';

import { PanelWrapper } from '../layout/PanelWrapper';
import { selectFvLastFetch, useFvStore } from '../../stores';
import { STALE_THRESHOLDS } from '../../lib/tokens';

const FV = lazy(() => import('../../FV'));

export const FVPanel = ({
  title = 'FV',
  onRemove,
  onCollapsedChange,
  fullWidth,
  collapsed,
}: {
  title?: string;
  onRemove?: () => void;
  onCollapsedChange?: (collapsed: boolean) => void;
  fullWidth?: boolean;
  collapsed?: boolean;
}) => {
  const lastUpdate = useFvStore(selectFvLastFetch);

  return (
    <PanelWrapper
      title={title}
      fullPageUrl="/fv"
      onRemove={onRemove}
      onCollapsedChange={onCollapsedChange}
      lastUpdate={lastUpdate}
      staleThresholdMs={STALE_THRESHOLDS.FAST}
      fullWidth={fullWidth}
      collapsed={collapsed}
    >
      <Suspense fallback={<div />}>
        <FV dense showHeader={false} />
      </Suspense>
    </PanelWrapper>
  );
};

(FVPanel as any).displayName = 'FV';
(FVPanel as any).defaultSize = { w: 12, h: 5 };
