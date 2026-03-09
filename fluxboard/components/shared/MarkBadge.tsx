/**
 * MarkBadge - Color-coded badge for displaying mark prices
 * Shows premium/discount status with subtle color coding
 */

import { colors, spacing, typography, borderRadius } from '@/lib/tokens';
import { fmtBalanceMark } from '@/utils';

// Tolerance for considering mark as neutral (≈1.00)
const MARK_NEUTRAL_TOLERANCE = 0.001;

type MarkBadgeProps = {
  mark: number;
};

export const MarkBadge = ({ mark }: MarkBadgeProps) => {
  const getMarkStyle = () => {
    const diff = Math.abs(mark - 1.0);
    if (diff < MARK_NEUTRAL_TOLERANCE) {
      // Neutral: mark ≈ 1.00
      return {
        backgroundColor: `${colors.neutral[700]}`,
        color: colors.text.secondary,
      };
    } else if (mark < 1.0) {
      // Red-tinted: mark < 1.00 (trading at discount)
      return {
        backgroundColor: `${colors.semantic.danger.DEFAULT}1a`,
        color: colors.semantic.danger.light,
      };
    } else {
      // Green-tinted: mark > 1.00 (trading at premium)
      return {
        backgroundColor: `${colors.semantic.success.DEFAULT}1a`,
        color: colors.semantic.success.light,
      };
    }
  };

  return (
    <span
      style={{
        ...getMarkStyle(),
        padding: `${spacing.padding.xs} ${spacing.padding.sm}`,
        borderRadius: borderRadius.sm,
        fontFamily: typography.fontFamily.mono,
        fontSize: typography.fontSize.xs,
        fontWeight: typography.fontWeight.medium,
      }}
    >
      {fmtBalanceMark(mark)}
    </span>
  );
};
