import Params from '@/Params';

export function MobileParamsView() {
  return (
    <div className="h-full flex flex-col">
      <Params variant="mobile" showHeader={false} />
    </div>
  );
}
