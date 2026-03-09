import { useState } from 'react';
import clsx from 'clsx';
import { MobileTradesView } from './MobileTradesView';
import { MobileParamsView } from './MobileParamsView';
import { colors, spacing, borderRadius, typography } from '@/lib/tokens';

const TABS = ['Trades', 'Params'] as const;
type Tab = (typeof TABS)[number];

export function MobileDashboard() {
  const [activeTab, setActiveTab] = useState<Tab>('Trades');

  return (
    <div
      className="flex flex-col h-screen"
      style={{ backgroundColor: colors.bg.base }}
      data-testid="mobile-dashboard"
    >
      <header
        className="shrink-0 flex gap-2 sticky top-0 z-20"
        style={{
          borderBottom: `1px solid ${colors.border.DEFAULT}`,
          padding: `${spacing.padding.sm} ${spacing.padding.md}`,
          backgroundColor: colors.bg.surface,
        }}
      >
        {TABS.map((tab) => {
          const isActive = tab === activeTab;
          return (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={clsx('flex-1 text-sm font-medium transition-colors')}
              style={{
                borderRadius: borderRadius.md,
                padding: `${spacing.padding.sm} ${spacing.gap.sm}`,
                backgroundColor: isActive ? colors.accent.DEFAULT : colors.bg.surfaceAlt,
                color: isActive ? colors.bg.base : colors.text.muted,
                border: `1px solid ${isActive ? colors.accent.DEFAULT : colors.border.hover}`,
                lineHeight: typography.lineHeight.snug,
              }}
            >
              {tab}
            </button>
          );
        })}
      </header>

      <main className="grow min-h-0" style={{ overflowY: 'auto', backgroundColor: colors.bg.surface }}>
        {activeTab === 'Trades' && <MobileTradesView />}
        {activeTab === 'Params' && <MobileParamsView />}
      </main>
    </div>
  );
}
