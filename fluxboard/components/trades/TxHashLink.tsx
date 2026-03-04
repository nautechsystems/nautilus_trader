// Transaction hash link component with copy-to-clipboard

import type { MouseEvent } from 'react';
import { shortHash } from './formatters';
import { colors } from '@/lib/tokens';
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard';

export const TxHashLink = ({
  hash,
  explorerUrl,
}: {
  hash?: string | null;
  explorerUrl?: string | null;
}) => {
  const copyToClipboard = useCopyToClipboard();

  if (!hash) {
    return (
      <span
        className="block"
        style={{ color: colors.text.muted }}
      >
        —
      </span>
    );
  }

  const shortTx = shortHash(hash);
  const href = typeof explorerUrl === 'string' && explorerUrl.startsWith('http') ? explorerUrl : null;

  const baseClass = 'block hover:underline transition-colors';
  const handleClick = (e: MouseEvent) => {
    e.stopPropagation();

    // If it's a left click and not opening in new tab, copy to clipboard
    if (e.button === 0 && !e.ctrlKey && !e.metaKey) {
      copyToClipboard(hash, { successMessage: 'Transaction hash copied' });

      // If there's an explorer URL, open it
      if (href) {
        window.open(href, '_blank', 'noopener,noreferrer');
      }
      e.preventDefault();
    }
  };

  const commonStyle = {
    color: colors.semantic.info.light,
    cursor: 'pointer',
  };

  if (!href) {
    // No explorer URL; clickable for copy only
    return (
      <span
        style={commonStyle}
        onClick={handleClick}
        title={`${hash} (click to copy)`}
        className={baseClass}
      >
        {shortTx}
      </span>
    );
  }

  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className={baseClass}
      style={commonStyle}
      onClick={handleClick}
      title={`${hash} (click to copy & open explorer)`}
    >
      {shortTx}
    </a>
  );
};
