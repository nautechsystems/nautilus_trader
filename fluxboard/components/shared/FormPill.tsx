/**
 * FormPill - Token form badge component
 *
 * Displays a small monochrome pill indicating token form:
 * - native: Canonical/unwrapped token
 * - wrapped: ERC-20 wrapped version
 * - bridged: Cross-chain bridged token
 * - staked: Staked/receipt token
 * - receipt: Receipt/voucher token
 * - other: Unknown/unmapped form
 *
 * Usage:
 *   <FormPill form="wrapped" />
 *   <FormPill form="native" />
 */

import React from 'react';

export type TokenForm = 'native' | 'wrapped' | 'bridged' | 'staked' | 'receipt' | 'other';

interface FormPillProps {
  /** Token form type */
  form: TokenForm;
  /** Optional additional className */
  className?: string;
}

/**
 * Get display label for form type
 */
function getFormLabel(form: TokenForm): string {
  switch (form) {
    case 'native':
      return 'native';
    case 'wrapped':
      return 'wrapped';
    case 'bridged':
      return 'bridged';
    case 'staked':
      return 'staked';
    case 'receipt':
      return 'receipt';
    case 'other':
    default:
      return 'other';
  }
}

export function FormPill({ form, className = '' }: FormPillProps) {
  // Don't render pill for 'other' form (graceful degradation)
  if (form === 'other') {
    return null;
  }

  const label = getFormLabel(form);

  return (
    <span
      className={`
        inline-block
        px-1.5
        py-0.5
        text-xs
        font-mono
        rounded
        bg-neutral-700
        text-neutral-300
        border
        border-neutral-600
        ${className}
      `.trim().replace(/\s+/g, ' ')}
      title={`Token form: ${label}`}
    >
      {label}
    </span>
  );
}
