import { Info } from 'lucide-react';

import { SimpleTooltip } from '@/components/ui/tooltip';
import type { FvSnapshot } from '@/types';
import {
  FV_HELP_ICON_CLASS,
  FV_SECTION_CARD_CLASS,
  FV_SECTION_TITLE_CLASS,
} from './styles';

const fmt = (value: number | null | undefined, digits = 6): string => {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  return value.toFixed(digits);
};

function TooltipLabel({ label, tooltip }: { label: string; tooltip: string }) {
  return (
    <span className={FV_SECTION_TITLE_CLASS}>
      <span>{label}</span>
      <SimpleTooltip content={tooltip} delay={200}>
        <button
          type="button"
          className={FV_HELP_ICON_CLASS}
          aria-label={`Help: ${label}`}
        >
          <Info className="h-3 w-3" />
        </button>
      </SimpleTooltip>
    </span>
  );
}

export function FVTable({ snapshot }: { snapshot?: FvSnapshot }) {
  if (!snapshot) {
    return (
      <div className={`${FV_SECTION_CARD_CLASS} text-sm text-text-muted`}>
        No FV snapshot available.
      </div>
    );
  }

  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
      <div className={FV_SECTION_CARD_CLASS}>
        <TooltipLabel
          label="Final FV"
          tooltip="Final fair value after applying signed-volume overlay to Base FV."
        />
        <div className="mt-2 font-mono text-lg font-semibold tabular-nums text-text-primary">{fmt(snapshot.final)}</div>
      </div>
      <div className={FV_SECTION_CARD_CLASS}>
        <TooltipLabel
          label="Base FV"
          tooltip="Weighted base fair value from term contributions before overlay."
        />
        <div className="mt-2 font-mono text-lg font-semibold tabular-nums text-text-primary">{fmt(snapshot.base)}</div>
      </div>
      <div className={FV_SECTION_CARD_CLASS}>
        <TooltipLabel
          label="Signed Volume"
          tooltip="Stateful signed-volume signal in [-1, 1], updated on trades and decayed over time."
        />
        <div className="mt-2 font-mono text-lg font-semibold tabular-nums text-text-primary">{fmt(snapshot.signed_volume, 4)}</div>
      </div>
      <div className={FV_SECTION_CARD_CLASS}>
        <TooltipLabel
          label="Overlay %"
          tooltip="Percent adjustment applied to Base FV from signed volume. Final FV = Base FV * (1 + Overlay%)."
        />
        <div className="mt-2 font-mono text-lg font-semibold tabular-nums text-text-primary">{fmt((snapshot.overlay_pct || 0) * 100, 2)}%</div>
      </div>
    </div>
  );
}
