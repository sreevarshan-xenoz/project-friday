import { Panel, Chip } from '../console/ui';
import { useConsole } from '../console/store';
import { fmtCost, fmtDateTime } from '../console/format';
import './ConsoleLedgerScreen.css';

export function ConsoleLedgerScreen() {
  const { state } = useConsole();

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">Ledger</h2>
          <p className="con-subtitle">Immutable, append-only decision history · {state.count} records</p>
        </div>
        <Chip tone="default">source: {state.source} · {new Date(state.generated_at).toLocaleTimeString([], { hour12: false })}</Chip>
      </header>

      <Panel title="Decision records">
        <div className="con-table-wrap">
          <table className="con-table">
            <thead>
              <tr>
                <th>#</th>
                <th>ID</th>
                <th>Time</th>
                <th>Capability</th>
                <th>Operator</th>
                <th>Est. latency</th>
                <th>Actual</th>
                <th>Est. cost</th>
                <th>Actual cost</th>
                <th>Status</th>
                <th>Retries</th>
              </tr>
            </thead>
            <tbody>
              {state.timelines.map((t, i) => (
                <tr key={t.correlation_id}>
                  <td className="con-table-idx">{i + 1}</td>
                  <td className="con-table-mono">{t.correlation_id}</td>
                  <td>{fmtDateTime(t.record.timestamp_ms)}</td>
                  <td className="con-table-mono">{t.plan.capability}</td>
                  <td>{t.plan.operator_id}</td>
                  <td className="con-table-num">{t.prediction.estimated_latency_ms} ms</td>
                  <td className="con-table-num">{t.outcome.latency_ms} ms</td>
                  <td className="con-table-num">{fmtCost(t.prediction.estimated_cost)}</td>
                  <td className="con-table-num">{fmtCost(t.outcome.cost)}</td>
                  <td>
                    <Chip tone={t.outcome.success ? 'green' : 'red'}>{t.outcome.success ? 'success' : 'failed'}</Chip>
                  </td>
                  <td className="con-table-num">{t.outcome.retries}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Panel>
    </div>
  );
}
