import { type ReactNode } from 'react';
import { cn } from '@/lib/utils';

type PageShellProps = {
  children: ReactNode;
  className?: string;
  fullHeight?: boolean;
};

export function PageShell({ children, className, fullHeight = true }: PageShellProps) {
  return (
    <div className={cn('page-shell', fullHeight && 'h-full', 'flex flex-col overflow-hidden', className)}>
      {children}
    </div>
  );
}
