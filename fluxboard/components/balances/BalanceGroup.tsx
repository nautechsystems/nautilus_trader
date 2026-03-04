/**
 * BalanceGroup - Token group with expandable children
 */

import { useCallback, type MouseEvent } from 'react';
import { motion, AnimatePresence, useReducedMotion } from 'framer-motion';
import { ChevronRight } from 'lucide-react';
import { colors, spacing, typography, borderRadius, animation } from '@/lib/tokens';
import { fmtBalanceQty, fmtBalanceMV, fmtBalanceMark } from '@/utils';
import { formatLocal, formatRelativeTime } from '@/utils/time';
import { BalanceRow } from './BalanceRow';
import { useFlashOnChange } from '@/hooks/useFlashOnChange';
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard';

type BalanceChild = {
  exchange: string;
  qty: number;
  mv: number;
  mark: number;
  update_time?: string;
};

type BalanceGroupProps = {
  coin: string;
  qty: number;
  mv: number;
  mark: number;
  updateTime?: string;
  children: BalanceChild[];
  expanded: boolean;
  onToggle: () => void;
};

export const BalanceGroup = ({
  coin,
  qty,
  mv,
  mark,
  updateTime,
  children,
  expanded,
  onToggle,
}: BalanceGroupProps) => {
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

  const handleQtyClick = useCallback((e: MouseEvent<HTMLDivElement>) => {
    e.stopPropagation();
    copyBalanceValue(qty.toString(), 'quantity');
  }, [qty, copyBalanceValue]);

  const handleMvClick = useCallback((e: MouseEvent<HTMLDivElement>) => {
    e.stopPropagation();
    copyBalanceValue(mv.toString(), 'market value');
  }, [mv, copyBalanceValue]);

  const hasChildren = children.length > 0;

  return (
    <div style={{ position: 'relative' }}>
      {/* Main row */}
      <div
        onClick={hasChildren ? onToggle : undefined}
        style={{
          display: 'grid',
          gridTemplateColumns: hasChildren
            ? '20px minmax(160px, 1fr) minmax(160px, auto) minmax(160px, auto) minmax(90px, auto) minmax(130px, auto)'
            : 'minmax(160px, 1fr) minmax(160px, auto) minmax(160px, auto) minmax(90px, auto) minmax(130px, auto)',
          gap: `0 ${spacing.gap.md}`,
          padding: `${spacing.padding.sm} ${spacing.gap.md}`,
          fontSize: typography.fontSize.sm,
          cursor: hasChildren ? 'pointer' : 'default',
          transition: `background-color ${animation.duration.fast}`,
          borderBottom: expanded && hasChildren ? 'none' : `1px solid ${colors.border.DEFAULT}`,
        }}
        className="hover:bg-surface-3/20"
      >
        {hasChildren && (
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <motion.div
              animate={{ rotate: expanded ? 90 : 0 }}
              transition={{ duration: 0.15, ease: 'easeInOut' }}
              style={{
                display: 'flex',
                alignItems: 'center',
                color: colors.text.muted,
              }}
            >
              <ChevronRight size={16} />
            </motion.div>
          </div>
        )}
        <div
          style={{
            color: colors.text.primary,
            fontWeight: typography.fontWeight.semibold,
          }}
        >
          {coin}
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
            color: colors.text.primary,
          }}
          title="Click to copy"
        >
          {fmtBalanceMV(mv)}
        </div>
        <div
          style={{
            textAlign: 'right',
            fontFamily: typography.fontFamily.mono,
            fontVariantNumeric: 'tabular-nums',
            color: colors.text.primary,
          }}
        >
          {fmtBalanceMark(mark)}
        </div>
        <div
          style={{
            textAlign: 'right',
            fontFamily: typography.fontFamily.mono,
            fontVariantNumeric: 'tabular-nums',
            color: colors.text.muted,
            fontSize: typography.fontSize.xs,
          }}
          title={updateTime ? formatLocal(updateTime) : ''}
        >
          {updateTime ? formatRelativeTime(updateTime) : '—'}
        </div>
      </div>

      {/* Expanded children */}
      <AnimatePresence>
        {expanded && hasChildren && (
            <motion.div
              initial={prefersReducedMotion ? undefined : { height: 0, opacity: 0 }}
              animate={prefersReducedMotion ? undefined : { height: 'auto', opacity: 1 }}
              exit={prefersReducedMotion ? undefined : { height: 0, opacity: 0 }}
              transition={prefersReducedMotion ? undefined : { duration: 0.2, ease: 'easeInOut' }}
              style={{
                overflow: 'hidden',
                transition: `all ${animation.duration.fast}`,
              }}
            >
            {/* Vertical connector line */}
            <div
              style={{
                position: 'absolute',
                left: '30px',
                top: 0,
                bottom: 0,
                width: '1px',
                backgroundColor: 'rgba(255, 255, 255, 0.08)',
              }}
            />

            {/* Child rows - simplified with only exchange and qty */}
            <AnimatePresence mode="popLayout">
              {children.map((child) => (
                <motion.div
                  key={`${coin}-${child.exchange}`}
                  initial={prefersReducedMotion ? undefined : { opacity: 0 }}
                  animate={prefersReducedMotion ? undefined : { opacity: 1 }}
                  exit={prefersReducedMotion ? undefined : { opacity: 0 }}
                  transition={prefersReducedMotion ? undefined : { duration: 0.2, ease: 'easeInOut' }}
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '20px minmax(160px, 1fr) minmax(160px, auto)',
                    gap: `0 ${spacing.gap.md}`,
                    padding: `${spacing.padding.sm} ${spacing.gap.md}`,
                    fontSize: typography.fontSize.xs,
                    transition: `background-color ${animation.duration.fast}`,
                  }}
                  className="hover:bg-surface-3/20"
                >
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                      color: colors.text.muted,
                    }}
                  >
                    •
                  </div>
                  <div
                    style={{
                      color: colors.text.secondary,
                      paddingLeft: spacing.gap.sm,
                    }}
                  >
                    {child.exchange}
                  </div>
                  <div
                    style={{
                      textAlign: 'right',
                      fontFamily: typography.fontFamily.mono,
                      fontVariantNumeric: 'tabular-nums',
                      color: colors.text.primary,
                    }}
                  >
                    {fmtBalanceQty(child.qty)}
                  </div>
                </motion.div>
              ))}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};
