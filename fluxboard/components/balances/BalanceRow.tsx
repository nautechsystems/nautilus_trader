/**
 * BalanceRow - Individual balance row component with click-to-copy and hover effects
 */

import { useCallback } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { colors, spacing, typography, borderRadius, animation } from '@/lib/tokens';
import { fmtBalanceQty, fmtBalanceMV } from '@/utils';
import { formatLocal, formatRelativeTime } from '@/utils/time';
import { MarkBadge } from '../shared/MarkBadge';
import { useFlashOnChange } from '@/hooks/useFlashOnChange';
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard';

type BalanceRowProps = {
  exchange: string;
  qty: number;
  mv: number;
  mark: number;
  updateTime?: string;
  isNested?: boolean;
};

export const BalanceRow = ({
  exchange,
  qty,
  mv,
  mark,
  updateTime,
  isNested = false,
}: BalanceRowProps) => {
  const prefersReducedMotion = useReducedMotion();
  const { flash: flashQty } = useFlashOnChange(qty);

  const copyToClipboard = useCopyToClipboard();

  const copyBalanceValue = useCallback((value: string, label: string) => {
    void copyToClipboard(value, {
      successMessage: `Copied ${label}`,
      errorMessage: `Failed to copy ${label}`,
      showPreview: false,
      successToastOptions: { duration: 1200 },
    });
  }, [copyToClipboard]);

  const handleQtyClick = useCallback(() => {
    copyBalanceValue(qty.toString(), 'quantity');
  }, [qty, copyBalanceValue]);

  const handleMvClick = useCallback(() => {
    copyBalanceValue(mv.toString(), 'market value');
  }, [mv, copyBalanceValue]);

  return (
    <motion.div
      initial={prefersReducedMotion ? undefined : { opacity: 0, y: -5 }}
      animate={prefersReducedMotion ? undefined : { opacity: 1, y: 0 }}
      exit={prefersReducedMotion ? undefined : { opacity: 0, y: -5 }}
      transition={prefersReducedMotion ? undefined : { duration: 0.15 }}
      style={{
        display: 'grid',
        gridTemplateColumns: isNested
          ? '40px 1fr 160px 160px 88px 170px'
          : '1fr 160px 160px 88px 170px',
        gap: `0 ${spacing.gap.md}`,
        padding: `${spacing.padding.sm} 0`,
        fontSize: typography.fontSize.xs,
        cursor: 'default',
        transition: `background-color ${animation.duration.fast}`,
      }}
      className="hover:bg-surface-3/50"
    >
      {isNested && <div />}
      <div
        style={{
          color: colors.text.secondary,
          paddingLeft: isNested ? spacing.gap.md : '0',
        }}
      >
        {exchange}
      </div>
      <div
        onClick={handleQtyClick}
        style={{
          textAlign: 'right',
          fontFamily: typography.fontFamily.mono,
          fontVariantNumeric: 'tabular-nums',
          cursor: 'pointer',
          padding: `0 ${spacing.padding.dense}`,
          borderRadius: borderRadius.sm,
          transition: `all ${animation.duration.fast}`,
          backgroundColor:
            flashQty === 'increase'
              ? `${colors.semantic.success.DEFAULT}33`
              : flashQty === 'decrease'
              ? `${colors.semantic.danger.DEFAULT}33`
              : 'transparent',
        }}
        title="Click to copy"
      >
        {fmtBalanceQty(qty)}
      </div>
      <div
        onClick={handleMvClick}
        style={{
          textAlign: 'right',
          fontFamily: typography.fontFamily.mono,
          fontVariantNumeric: 'tabular-nums',
          cursor: 'pointer',
          padding: `0 ${spacing.padding.dense}`,
          borderRadius: borderRadius.sm,
          transition: `all ${animation.duration.fast}`,
        }}
        title="Click to copy"
      >
        {fmtBalanceMV(mv)}
      </div>
      <div
        style={{
          textAlign: 'right',
          display: 'flex',
          justifyContent: 'flex-end',
        }}
      >
        <MarkBadge mark={mark} />
      </div>
      <div
        style={{
          textAlign: 'right',
          fontFamily: typography.fontFamily.mono,
          fontVariantNumeric: 'tabular-nums',
          color: colors.text.muted,
        }}
        title={updateTime ? formatLocal(updateTime) : ''}
      >
        {updateTime ? formatRelativeTime(updateTime) : '—'}
      </div>
    </motion.div>
  );
};
