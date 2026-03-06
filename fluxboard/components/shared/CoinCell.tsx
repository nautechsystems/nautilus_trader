/**
 * CoinCell - Composite coin display with form pill and metadata tooltip
 *
 * Displays:
 * - Symbol (canonical)
 * - FormPill (only for child rows)
 * - Venue (exchange/wallet)
 * - Wallet label (if applicable)
 * - Tooltip: Full metadata (chain, contract, form, decimals)
 * - Click-to-copy contract address
 *
 * Usage:
 *   <CoinCell
 *     symbol="PLUME"
 *     form="wrapped"
 *     venue="wallet"
 *     walletLabel="wplume"
 *     chain="plume"
 *     contract="0x..."
 *     isChild={true}
 *   />
 */

import React, { useState } from 'react';
import { FormPill, type TokenForm } from './FormPill';
import { SimpleTooltip } from '../ui/tooltip';

interface CoinCellProps {
  /** Token symbol (canonical) */
  symbol: string;
  /** Chain name */
  chain?: string | null;
  /** Token form (native/wrapped/etc.) */
  form?: TokenForm;
  /** Venue/exchange */
  venue?: string | null;
  /** Wallet label */
  walletLabel?: string | null;
  /** Contract address */
  contract?: string | null;
  /** Whether this is a child row (determines if FormPill renders) */
  isChild?: boolean;
  /** Optional additional className */
  className?: string;
}

/**
 * Shorten contract address for display
 */
function shortenAddress(address: string): string {
  if (address.length <= 10) {
    return address;
  }
  return `${address.slice(0, 6)}...${address.slice(-4)}`;
}

function isAddressLike(value: string): boolean {
  const text = value.trim();
  if (!text) return false;
  if (text.startsWith('0x')) return true;
  return /^[A-Fa-f0-9]{24,}$/.test(text);
}

/**
 * Copy text to clipboard
 */
async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
    // Fallback for older browsers
    const textarea = document.createElement('textarea');
    textarea.value = text;
    textarea.style.position = 'fixed';
    textarea.style.opacity = '0';
    document.body.appendChild(textarea);
    textarea.select();
    const success = document.execCommand('copy');
    document.body.removeChild(textarea);
    return success;
  } catch (error) {
    console.error('Failed to copy to clipboard:', error);
    return false;
  }
}

export function CoinCell({
  symbol,
  chain,
  form = 'other',
  venue,
  walletLabel,
  contract,
  isChild = false,
  className = '',
}: CoinCellProps) {
  const [copyFeedback, setCopyFeedback] = useState<string | null>(null);
  const contractText = String(contract ?? '').trim();
  const contractIsAddress = isAddressLike(contractText);

  // Build tooltip content with metadata (memoized)
  const tooltipContent = React.useMemo(() => {
    const lines: string[] = [];

    if (chain) {
      lines.push(`${symbol}.${chain}`);
    } else {
      lines.push(symbol);
    }

    if (contract) {
      lines.push(`• ${contract}`);
    }

    if (form && form !== 'other') {
      lines.push(`• form=${form}`);
    }

    return lines.join('\n');
  }, [symbol, chain, contract, form]);

  // Handle contract address click-to-copy
  const handleContractClick = async (e: React.MouseEvent) => {
    if (!contract) return;

    e.stopPropagation();
    const success = await copyToClipboard(contract);

    if (success) {
      setCopyFeedback('Copied!');
      setTimeout(() => setCopyFeedback(null), 2000);
    } else {
      setCopyFeedback('Failed');
      setTimeout(() => setCopyFeedback(null), 2000);
    }
  };

  return (
    <div className={`flex items-center gap-2 ${className}`}>
      {/* Symbol */}
      <SimpleTooltip content={tooltipContent}>
        <span className="font-semibold text-neutral-100">
          {symbol}
        </span>
      </SimpleTooltip>

      {/* Form pill (only for child rows) */}
      {isChild && form && form !== 'other' && (
        <FormPill form={form} />
      )}

      {/* Venue/wallet label */}
      {venue && (
        <span className="text-xs text-neutral-400">
          {venue}
          {walletLabel && ` (${walletLabel})`}
          {!contractIsAddress && contractText && ` ${contractText}`}
        </span>
      )}

      {/* Contract address (click to copy) */}
      {contractIsAddress && contractText && (
        <button
          onClick={handleContractClick}
          className="
            text-xs
            text-neutral-500
            hover:text-neutral-300
            font-mono
            cursor-pointer
            transition-colors
            relative
          "
          title="Click to copy contract address"
        >
          {copyFeedback ? (
            <span className="text-emerald-400 font-semibold">
              {copyFeedback}
            </span>
          ) : (
            shortenAddress(contractText)
          )}
        </button>
      )}
    </div>
  );
}
