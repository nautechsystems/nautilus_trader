// FX status pill component

import type { FxPair } from './types';
import { deriveFxStatus } from './utils';
import { severity, spacing, borderRadius, typography, colors } from './lib/tokens';

type Props = {
  pair: FxPair;
};

const STATUS_TOKENS = {
  green: severity.success,
  yellow: severity.warning,
  red: severity.critical,
} as const;

export default function FxStatusPill({ pair }: Props) {
  const status = deriveFxStatus(pair);
  const palette = STATUS_TOKENS[status] ?? {
    color: colors.text.secondary,
    bg: colors.bg.surface,
    border: colors.border.DEFAULT,
  };

  let label = 'OK';
  if (status === 'yellow') label = 'FALLBACK';
  if (status === 'red') label = pair.stale ? 'STALE' : 'BREACH';

  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontFamily: typography.fontFamily.mono,
        fontSize: typography.fontSize.xs,
        fontWeight: typography.fontWeight.semibold,
        padding: `${spacing.padding.xs} ${spacing.gap.xs}`,
        borderRadius: borderRadius.full,
        letterSpacing: '0.06em',
        textTransform: 'uppercase',
        color: palette.text ?? palette.color,
        backgroundColor: palette.bg,
        border: `1px solid ${palette.border ?? palette.color}`,
      }}
    >
      {label}
    </span>
  );
}
