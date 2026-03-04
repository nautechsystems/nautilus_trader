// Decision JSON modal with tabbed interface

import { useState, useEffect } from 'react';
import type { Trade } from '../../types';
import { Dialog, DialogFooter } from '../ui/dialog/Dialog';
import { TabsRoot, TabsList, TabsTrigger, TabsContent } from '../ui/tabs/Tabs';
import { Button } from '../ui/button/Button';
import { colors } from '@/lib/tokens';

type DecisionData = {
  version?: string;
  summary?: string;
  decision_timestamp?: {
    iso?: string;
    unix_ms?: number;
  };
  market_data?: any;
  fair_values?: any;
  fees?: {
    gas_quote_per_unit?: number;
    leg1?: any;
    leg2?: any;
  };
  edge_parameters?: any;
  strategy_parameters?: any;
  opportunity?: {
    case?: number;
    spread_bps?: number;
    edge_bps_net?: number;
    required_bps?: number;
    gas_bps?: number;
    [key: string]: any;
  };
  [key: string]: any;
};

type Tab = 'summary' | 'legs' | 'fees' | 'params' | 'raw';

export const DecisionModal = ({
  trade,
  onClose,
}: {
  trade: Trade;
  onClose: () => void;
}) => {
  const [activeTab, setActiveTab] = useState<Tab>('summary');
  const [decision, setDecision] = useState<DecisionData | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);

  useEffect(() => {
    // Parse decision JSON
    if (trade.decision && typeof trade.decision === 'string') {
      try {
        const parsed = JSON.parse(trade.decision);
        setDecision(parsed);
        setParseError(null);
      } catch (e) {
        setParseError((e as Error).message);
      }
    } else if (trade.decision && typeof trade.decision === 'object') {
      setDecision(trade.decision as DecisionData);
    }
  }, [trade.decision]);

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text).catch(console.error);
  };

  const copyJSON = () => {
    if (decision) {
      copyToClipboard(JSON.stringify(decision, null, 2));
    }
  };

  const copySummary = () => {
    if (!decision?.opportunity) return;
    const { case: caseNum, spread_bps, edge_bps_net, required_bps, gas_bps } = decision.opportunity;
    const text = `case\t${caseNum}\nspread_bps\t${spread_bps}\nedge_bps_net\t${edge_bps_net}\nrequired_bps\t${required_bps}\ngas_bps\t${gas_bps || 0}`;
    copyToClipboard(text);
  };

  const tabs: { id: Tab; label: string }[] = [
    { id: 'summary', label: 'Summary' },
    { id: 'legs', label: 'Legs' },
    { id: 'params', label: 'Params' },
    { id: 'raw', label: 'Raw' },
  ];

  return (
    <Dialog
      isOpen={true}
      onClose={onClose}
      title={`Decision: ${trade.trade_id.slice(0, 8)}...`}
      size="xl"
      variant="sheet"
      className="max-w-4xl"
      footer={
        <DialogFooter>
          <Button
            variant="secondary"
            size="sm"
            onClick={copyJSON}
          >
            Copy JSON
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={onClose}
          >
            Close
          </Button>
        </DialogFooter>
      }
    >
      <TabsRoot value={activeTab} onValueChange={(v) => setActiveTab(v as Tab)} className="flex flex-col h-full overflow-hidden">
        <TabsList
          className="flex border-b mb-4"
          style={{ borderColor: colors.border.DEFAULT }}
        >
          {tabs.map((tab) => (
            <TabsTrigger
              key={tab.id}
              value={tab.id}
              className="px-4 py-2 text-sm font-medium border-b-2 border-transparent transition-colors data-[state=active]:border-emerald-500"
              style={{
                color: colors.text.secondary,
              }}
            >
              {tab.label}
            </TabsTrigger>
          ))}
        </TabsList>

        {parseError && (
          <div
            className="rounded p-3 text-sm mb-4"
            style={{
              backgroundColor: colors.semantic.danger.bg,
              border: `1px solid ${colors.semantic.danger.darker}`,
              color: colors.semantic.danger.light,
            }}
          >
            Error parsing decision JSON: {parseError}
          </div>
        )}

        {!decision && !parseError && (
          <div className="text-sm" style={{ color: colors.text.muted }}>
            No decision data available
          </div>
        )}

        {decision && (
          <>
            <TabsContent value="summary" className="overflow-y-auto h-full">
              <SummaryTab decision={decision} onCopy={copySummary} />
            </TabsContent>
            <TabsContent value="legs" className="overflow-y-auto h-full">
              <LegsTab decision={decision} />
            </TabsContent>
            <TabsContent value="params" className="overflow-y-auto h-full">
              <ParamsTab decision={decision} />
            </TabsContent>
            <TabsContent value="raw" className="overflow-y-auto h-full">
              <RawTab decision={decision} />
            </TabsContent>
          </>
        )}
      </TabsRoot>
    </Dialog>
  );
};

// Summary tab component
const SummaryTab = ({ decision, onCopy }: { decision: DecisionData; onCopy: () => void }) => {
  const opp = decision.opportunity;
  if (!opp) return <div className="text-sm" style={{ color: colors.text.muted }}>No opportunity data</div>;

  const fees = decision.fees;

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-start">
        <h3 className="text-sm font-medium" style={{ color: colors.text.secondary }}>
          Key Metrics
        </h3>
        <Button
          variant="secondary"
          size="xs"
          onClick={onCopy}
        >
          Copy TSV
        </Button>
      </div>
      <div className="grid grid-cols-2 gap-4">
        <MetricRow label="Case" value={opp.case} />
        <MetricRow label="Spread (bps)" value={opp.spread_bps?.toFixed(2)} />
        <MetricRow label="Edge Net (bps)" value={opp.edge_bps_net?.toFixed(2)} />
        <MetricRow label="Required (bps)" value={opp.required_bps?.toFixed(2)} />
        <MetricRow label="Gas (bps)" value={opp.gas_bps?.toFixed(2) || '0.00'} />
        <MetricRow label="Leg 1 Action" value={opp.leg1_action} />
        <MetricRow label="Leg 2 Action" value={opp.leg2_action} />
        {fees?.gas_quote_per_unit !== undefined && (
          <MetricRow label="Gas Quote / Unit" value={fees.gas_quote_per_unit} />
        )}
        {fees?.leg1?.taker_fee_bps !== undefined && (
          <MetricRow label="Leg 1 Taker Fee (bps)" value={fees.leg1.taker_fee_bps} />
        )}
        {fees?.leg2?.taker_fee_bps !== undefined && (
          <MetricRow label="Leg 2 Taker Fee (bps)" value={fees.leg2.taker_fee_bps} />
        )}
        {fees?.leg1?.pool_fee_bps !== undefined && (
          <MetricRow label="Leg 1 Pool Fee (bps)" value={fees.leg1.pool_fee_bps} />
        )}
        {fees?.leg2?.pool_fee_bps !== undefined && (
          <MetricRow label="Leg 2 Pool Fee (bps)" value={fees.leg2.pool_fee_bps} />
        )}
      </div>
      {decision.summary && (
        <div
          className="mt-4 p-3 rounded"
          style={{
            backgroundColor: colors.bg.hover,
            border: `1px solid ${colors.border.DEFAULT}`,
          }}
        >
          <div className="text-xs mb-1" style={{ color: colors.text.muted }}>
            Summary
          </div>
          <div className="text-sm" style={{ color: colors.text.secondary }}>
            {decision.summary}
          </div>
        </div>
      )}
    </div>
  );
};

// Legs tab component
const LegsTab = ({ decision }: { decision: DecisionData }) => {
  const md = decision.market_data;
  const fv = decision.fair_values;
  if (!md && !fv) return <div className="text-sm" style={{ color: colors.text.muted }}>No legs data</div>;

  return (
    <div className="space-y-4">
      {md?.leg1 && (
        <LegCard title="Leg 1 (Market Data)" data={md.leg1} fvData={fv?.leg1} />
      )}
      {md?.leg2 && (
        <LegCard title="Leg 2 (Market Data)" data={md.leg2} fvData={fv?.leg2} />
      )}
    </div>
  );
};

const LegCard = ({ title, data, fvData }: { title: string; data: any; fvData?: any }) => (
  <div
    className="p-3 rounded"
    style={{
      backgroundColor: colors.bg.hover,
      border: `1px solid ${colors.border.DEFAULT}`,
    }}
  >
    <div className="text-sm font-medium mb-2" style={{ color: colors.text.secondary }}>
      {title}
    </div>
    <div className="grid grid-cols-2 gap-2 text-xs">
      <MetricRow label="Exchange" value={data.exchange} />
      <MetricRow label="Symbol" value={data.symbol} />
      <MetricRow label="Type" value={data.type} />
      <MetricRow label="Age (ms)" value={data.age_ms} />
      {data.raw?.bid !== null && <MetricRow label="Raw Bid" value={data.raw?.bid} />}
      {data.raw?.ask !== null && <MetricRow label="Raw Ask" value={data.raw?.ask} />}
      {data.raw?.mid !== null && <MetricRow label="Raw Mid" value={data.raw?.mid} />}
      {fvData?.fv_bid !== null && <MetricRow label="FV Bid" value={fvData?.fv_bid} />}
      {fvData?.fv_ask !== null && <MetricRow label="FV Ask" value={fvData?.fv_ask} />}
    </div>
  </div>
);

// Fees tab component
const FeesTab = ({ decision }: { decision: DecisionData }) => {
  const fees = decision.fees;
  if (!fees) return <div className="text-sm" style={{ color: colors.text.muted }}>No fees data</div>;

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-4">
        <MetricRow label="Gas Quote Per Unit" value={fees.gas_quote_per_unit} />
      </div>
      {fees.leg1 && <FeeCard title="Leg 1 Fees" data={fees.leg1} />}
      {fees.leg2 && <FeeCard title="Leg 2 Fees" data={fees.leg2} />}
    </div>
  );
};

const FeeCard = ({ title, data }: { title: string; data: any }) => (
  <div
    className="p-3 rounded"
    style={{
      backgroundColor: colors.bg.hover,
      border: `1px solid ${colors.border.DEFAULT}`,
    }}
  >
    <div className="text-sm font-medium mb-2" style={{ color: colors.text.secondary }}>
      {title}
    </div>
    <div className="grid grid-cols-2 gap-2 text-xs">
      {data.taker_fee_bps !== null && <MetricRow label="Taker Fee (bps)" value={data.taker_fee_bps} />}
      {data.maker_fee_bps !== null && <MetricRow label="Maker Fee (bps)" value={data.maker_fee_bps} />}
      {data.maker_rebate_bps !== null && <MetricRow label="Maker Rebate (bps)" value={data.maker_rebate_bps} />}
      {data.pool_fee_bps !== null && <MetricRow label="Pool Fee (bps)" value={data.pool_fee_bps} />}
    </div>
  </div>
);

// Params tab component
const ParamsTab = ({ decision }: { decision: DecisionData }) => {
  const edge = decision.edge_parameters;
  const strategy = decision.strategy_parameters;
  if (!edge && !strategy) return <div className="text-sm" style={{ color: colors.text.muted }}>No params data</div>;

  return (
    <div className="space-y-4">
      {edge && (
        <div
          className="p-3 rounded"
          style={{
            backgroundColor: colors.bg.hover,
            border: `1px solid ${colors.border.DEFAULT}`,
          }}
        >
          <div className="text-sm font-medium mb-2" style={{ color: colors.text.secondary }}>
            Edge Parameters
          </div>
          <div className="grid grid-cols-2 gap-2 text-xs">
            {Object.entries(edge).map(([key, value]) => (
              <MetricRow key={key} label={key} value={value} />
            ))}
          </div>
        </div>
      )}
      {strategy && (
        <div
          className="p-3 rounded"
          style={{
            backgroundColor: colors.bg.hover,
            border: `1px solid ${colors.border.DEFAULT}`,
          }}
        >
          <div className="text-sm font-medium mb-2" style={{ color: colors.text.secondary }}>
            Strategy Parameters
          </div>
          <div className="grid grid-cols-2 gap-2 text-xs">
            {Object.entries(strategy).map(([key, value]) => (
              <MetricRow key={key} label={key} value={value} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

// Raw tab component
const RawTab = ({ decision }: { decision: DecisionData }) => (
  <pre
    className="text-xs rounded p-4"
    style={{
      backgroundColor: colors.bg.hover,
      border: `1px solid ${colors.border.DEFAULT}`,
    }}
  >
    {JSON.stringify(decision, null, 2)}
  </pre>
);

// Metric row component
const MetricRow = ({ label, value }: { label: string; value: any }) => (
  <div>
    <div className="text-xs" style={{ color: colors.text.muted }}>
      {label}
    </div>
    <div className="text-sm font-mono" style={{ color: colors.text.secondary }}>
      {value !== null && value !== undefined ? String(value) : '—'}
    </div>
  </div>
);
