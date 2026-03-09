import Trades from '@/Trades';

export function MobileTradesView() {
  return (
    <div className="h-full flex flex-col">
      <Trades variant="mobile" showHeader={false} />
    </div>
  );
}
