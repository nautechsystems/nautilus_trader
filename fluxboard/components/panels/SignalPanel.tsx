// Signal panel component for dashboard

import { PanelWrapper } from '../layout/PanelWrapper';
import SignalTable from '../domain/signal/SignalTable';
import { useSignalStore, selectSignalLastUpdate } from '../../stores';
import { STALE_THRESHOLDS } from '../../lib/tokens';

export const SignalPanel = ({
  title = 'Signal',
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
  // Optimized: only re-renders when lastUpdate changes, not when rows change
  const lastUpdate = useSignalStore(selectSignalLastUpdate);

  return (
    <PanelWrapper
      title={title}
      fullPageUrl="/signal"
      onRemove={onRemove}
      onCollapsedChange={onCollapsedChange}
      lastUpdate={lastUpdate}
      staleThresholdMs={STALE_THRESHOLDS.FAST}
      fullWidth={fullWidth}
      collapsed={collapsed}
    >
      <SignalTable showHeader={false} />
    </PanelWrapper>
  );
};

(SignalPanel as any).displayName = 'Signal';
(SignalPanel as any).defaultSize = { w: 12, h: 6, minW: 6, maxW: 12 };
