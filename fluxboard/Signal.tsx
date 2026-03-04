// Signal page - Strategy health and edge monitoring

import SignalTable from './components/domain/signal/SignalTable';
import { PageShell } from './components/layout/PageShell';

export default function Signal() {
  return (
    <PageShell>
      <SignalTable />
    </PageShell>
  );
}
