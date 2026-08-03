import { Panel, Stat, Chip, Bar, KeyVal } from '../console/ui';
import { useConsole } from '../console/store';
import { fmtPct, fmtMs, fmtTime, fmtBytes } from '../console/format';
import './ConsoleDashboardScreen.css';

export function ConsoleDashboardScreen() {
  const snap = useConsole();

  if (snap.status !== 'ready') {
    return (
      <div className="con-screen">
        <header className="con-head">
          <div>
            <h2 className="con-title">FRIDAY</h2>
            <p className="con-subtitle">Engineering Console · mission control</p>
          </div>
          <div className="con-head-right">
            <Chip tone="default">{snap.status === 'error' ? 'error' : 'loading'}</Chip>
          </div>
        </header>
        <p className="con-empty">
          {snap.status === 'error' ? snap.error : 'Connecting to data source…'}
        </p>
      </div>
    );
  }

  const { dashboard, runtime, count, timelines } = snap;
  const last = timelines[timelines.length - 1];

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">FRIDAY</h2>
          <p className="con-subtitle">Engineering Console · mission control</p>
        </div>
        <div className="con-head-right">
          <Chip tone="primary">{dashboard.phase}</Chip>
          <Chip tone="default">{runtime.bridge} · {count} decisions</Chip>
        </div>
      </header>

      <section className="con-goal">
        <div className="con-goal-main">
          <span className="con-goal-label">Active goal</span>
          <span className="con-goal-text">{dashboard.mission}</span>
        </div>
        <div className="con-goal-progress">
          <Bar value={dashboard.goal_progress} tone="primary" label={fmtPct(dashboard.goal_progress, 0)} />
        </div>
      </section>

      <div className="con-grid con-grid--dash">
        <Panel title="Executive">
          <div className="con-stages">
            {dashboard.executive.map((p) => (
              <div key={p.name} className="con-stage">
                <span className={`con-stage-dot con-stage-dot--${p.status}`} />
                <span className="con-stage-name">{p.name}</span>
                <span className="con-stage-ms">{p.status === 'running' ? 'now' : ''}</span>
              </div>
            ))}
          </div>
        </Panel>

        <Panel title="World state">
          <div className="con-world">
            <Stat label="Files" value={dashboard.world.files} />
            <Stat label="Processes" value={dashboard.world.processes} />
            <Stat label="Capabilities" value={dashboard.world.capabilities} />
            <Stat label="Policies" value={dashboard.world.policies} />
            <Stat label="Episodes" value={dashboard.world.episodes} />
            <Stat label="Confidence" value={fmtPct(dashboard.world.confidence)} tone={dashboard.world.confidence >= 0.85 ? 'green' : 'amber'} />
          </div>
        </Panel>

        <Panel title="Runtime">
          <div className="con-runtime">
            <KeyVal
              k="Connected"
              v={<Chip tone={runtime.connected ? 'green' : 'red'}>{runtime.connected ? 'YES' : 'NO'}</Chip>}
            />
            <KeyVal k="Bridge" v={runtime.bridge} />
            <KeyVal k="Protocol" v={`v${runtime.protocol}`} />
            <KeyVal k="Frames" v={runtime.frames} />
            <KeyVal k="Dropped" v={<Chip tone={runtime.dropped ? 'amber' : 'green'}>{runtime.dropped}</Chip>} />
            <KeyVal k="Reconnects" v={runtime.reconnects} />
            <KeyVal k="Avg FPS" v={runtime.fps.toFixed(1)} />
            <KeyVal k="Round trip" v={runtime.round_trip_ms == null ? '—' : `${runtime.round_trip_ms.toFixed(0)} ms`} />
            <KeyVal k="Avg RTT" v={runtime.avg_rtt_ms == null ? '—' : `${runtime.avg_rtt_ms.toFixed(1)} ms`} />
            <KeyVal k="Peak RTT" v={runtime.peak_rtt_ms == null ? '—' : `${runtime.peak_rtt_ms.toFixed(0)} ms`} />
            <KeyVal k="Snapshot" v={runtime.snapshot_bytes == null ? '—' : `${fmtBytes(runtime.snapshot_bytes)}`} />
            <KeyVal k="Delta" v={runtime.delta_bytes == null ? '—' : `${fmtBytes(runtime.delta_bytes)}`} />
            <KeyVal k="Records" v={runtime.records} />
            {runtime.last_error && (
              <KeyVal k="Error" v={<Chip tone="red">{runtime.last_error}</Chip>} />
            )}
          </div>
        </Panel>

        <Panel title="Latest decision">
          {last ? (
            <div className="con-latest">
              <div className="con-latest-cap">{last.plan.capability}</div>
              <div className="con-latest-op">{last.plan.operator_id}</div>
              <div className="con-latest-meta">
                <span>{fmtMs(last.outcome.latency_ms)}</span>
                <span>{last.outcome.success ? 'success' : 'failed'}</span>
                <span>{fmtTime(last.record.timestamp_ms)}</span>
              </div>
            </div>
          ) : (
            <p className="con-empty">No decisions yet</p>
          )}
        </Panel>

        <Panel title="Recent decisions">
          <div className="con-recent">
            {dashboard.recent.map((t) => (
              <div key={t.correlation_id} className="con-recent-row">
                <span className="con-recent-cap">{t.plan.capability}</span>
                <span className="con-recent-op">{t.plan.operator_id}</span>
                <Chip tone={t.outcome.success ? 'green' : 'red'}>{t.outcome.success ? 'OK' : 'FAIL'}</Chip>
                <span className="con-recent-time">{fmtTime(t.record.timestamp_ms)}</span>
              </div>
            ))}
          </div>
        </Panel>
      </div>
    </div>
  );
}
