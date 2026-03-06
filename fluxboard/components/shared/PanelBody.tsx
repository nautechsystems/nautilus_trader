import { forwardRef, type ReactNode } from 'react';
import { cn } from '@/lib/utils';

export const PanelBody = forwardRef<HTMLDivElement, { children: ReactNode; className?: string }>(({ children, className }, ref) => {
  return (
    <div className={cn("flex-1 overflow-hidden", className)}>
      <div className="h-full overflow-auto relative" ref={ref}>
        {children}
      </div>
    </div>
  );
});

PanelBody.displayName = 'PanelBody';
