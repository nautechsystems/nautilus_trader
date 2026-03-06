import { Info } from 'lucide-react';

import { SimpleTooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { FvTermBreakdown } from '@/types';
import {
  FV_HELP_ICON_CLASS,
  FV_SECTION_CARD_CLASS,
  FV_SECTION_TITLE_CLASS,
  FV_SUBSECTION_CLASS,
} from './styles';

const fmt = (value: number | null | undefined, digits = 6): string => {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  return value.toFixed(digits);
};

const fmtSigned = (value: number | null | undefined, digits = 9): string => {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  const sign = value >= 0 ? '+' : '-';
  return `${sign}${Math.abs(value).toFixed(digits)}`;
};

function ColumnHeaderWithTooltip({
  label,
  tooltip,
  align = 'right',
}: {
  label: string;
  tooltip: string;
  align?: 'left' | 'right';
}) {
  return (
    <span className={cn('inline-flex items-center gap-1', align === 'right' ? 'justify-end' : 'justify-start')}>
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

export function FVBreakdown({
  terms,
  selectedTermId,
  onSelectTerm,
}: {
  terms: FvTermBreakdown[];
  selectedTermId?: number;
  onSelectTerm?: (termId: number) => void;
}) {
  const orderedTerms = terms
    .slice()
    .sort((a, b) => a.id - b.id);
  const deltaClassFor = (value: number | null | undefined): string => {
    if ((value || 0) < 0) return 'text-danger-light';
    if ((value || 0) > 0) return 'text-success-light';
    return 'text-text-primary';
  };

  return (
    <div className={FV_SECTION_CARD_CLASS}>
      <div className={`mb-3 ${FV_SECTION_TITLE_CLASS}`}>
        <span>Term Breakdown</span>
        <SimpleTooltip content="Per-term decomposition used to build Base FV for the selected symbol/profile." delay={200}>
          <button
            type="button"
            className={FV_HELP_ICON_CLASS}
            aria-label="Help: Term Breakdown"
          >
            <Info className="h-3 w-3" />
          </button>
        </SimpleTooltip>
      </div>
      <div className={cn('overflow-x-auto', FV_SUBSECTION_CLASS)}>
        <table className="w-full min-w-[48rem] table-fixed text-sm">
          <colgroup>
            <col className="w-[40%]" />
            <col className="w-[14ch]" />
            <col className="w-[14ch]" />
            <col className="w-[16ch]" />
          </colgroup>
          <thead>
            <tr className="border-b border-border text-text-muted">
              <th className="px-3 py-2 text-left">
                <ColumnHeaderWithTooltip
                  label="Term"
                  tooltip="Configured term id/name with weight and trigger gate."
                  align="left"
                />
              </th>
              <th className="px-3 py-2 text-right whitespace-nowrap">
                <ColumnHeaderWithTooltip
                  label="Value"
                  tooltip="Term output value after its formula mode (raw/power/linear)."
                />
              </th>
              <th className="px-3 py-2 text-right whitespace-nowrap">
                <ColumnHeaderWithTooltip
                  label="Contribution"
                  tooltip="Weighted contribution of this term into Base FV."
                />
              </th>
              <th className="px-3 py-2 text-right whitespace-nowrap">
                <ColumnHeaderWithTooltip
                  label="Delta"
                  tooltip="Signed change in term contribution versus the prior update."
                />
              </th>
            </tr>
          </thead>
          <tbody>
            {orderedTerms.map((term) => (
              <tr
                key={term.id}
                className={cn(
                  'cursor-pointer border-t border-border text-text-secondary transition-colors hover:bg-bg-hover/60',
                  selectedTermId === term.id && 'bg-bg-active/60'
                )}
                onClick={() => onSelectTerm?.(term.id)}
              >
                <td className="px-3 py-2">
                  <div className="truncate font-medium text-text-primary">{term.name}</div>
                  <div className="text-[11px] text-text-muted">
                    w={fmt(term.weight, 1)} trigger={term.trigger || '—'}
                  </div>
                </td>
                <td className="px-3 py-2 text-right font-mono tabular-nums whitespace-nowrap text-text-primary">{fmt(term.value)}</td>
                <td className="px-3 py-2 text-right font-mono tabular-nums whitespace-nowrap text-text-primary">{fmt(term.contribution)}</td>
                <td className="px-3 py-2 text-right font-mono tabular-nums whitespace-nowrap">
                  <span
                    className={cn(
                      'inline-block w-[14ch] text-right',
                      deltaClassFor(term.contribution_delta)
                    )}
                  >
                    {fmtSigned(term.contribution_delta)}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
