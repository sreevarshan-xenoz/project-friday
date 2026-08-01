import { Panel, Chip } from '../console/ui';
import { useConsole } from '../console/store';
import { fmtCost, fmtDateTime } from '../console/format';
import './ConsoleLedgerScreen.css';

export function ConsoleLedgerScreen() {
  const { status, error, source, count, ledger } = useConsole();

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">Ledger</h2>
          <p className="con-subtitle">Immutable, append-only decision history · {count} records</p>
        </div>
        <Chip tone="default">source: {source}</Chip>
      </header>

      {status !== 'ready' ? (
        <p className="con-empty">{status === 'error' ? error : 'Loading read models…'}</p>
      ) : (
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
              {ledger.map((r, i) => (
                <tr key={r.id}>
                  <td className="con-table-idx">{i + 1}</td>
                  <td className="con-table-mono">{r.id}</td>
                  <td>{fmtDateTime(r.timestamp_ms)}</td>
                  <td className="con-table-mono">{r.capability}</td>
                  <td>{r.plan.operator_id}</td>
                  <td className="con-table-num">{r.plan.estimated_latency_ms} ms</td>
                  <td className="con-table-num">{r.outcome.latency_ms} ms</td>
                  <td className="con-table-num">{fmtCost(r.plan.estimated_cost)}</td>
                  <td className="con-table-num">{fmtCost(r.outcome.cost)}</td>
                  <td>
                    <Chip tone={r.outcome.success ? 'green' : 'red'}>{r.outcome.success ? 'success' : 'failed'}</Chip>
                  </td>
                  <td className="con-table-num">{r.outcome.retries}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Panel>
      )}
    </div>
  );
}
