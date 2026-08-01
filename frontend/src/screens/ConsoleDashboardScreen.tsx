import { Panel, Stat, Chip, Bar } from '../console/ui';
import { useConsole } from '../console/store';
import { fmtPct, fmtMs, fmtTime } from '../console/format';
import './ConsoleDashboardScreen.css';

export function ConsoleDashboardScreen() {
  const { overview, state } = useConsole();
  const last = state.timelines[state.timelines.length - 1];
  const recent = state.timelines.slice(-6).reverse();

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">FRIDAY</h2>
          <p className="con-subtitle">Engineering Console · mission control</p>
        </div>
        <div className="con-head-right">
          <Chip tone="primary">{overview.phase}</Chip>
          <Chip tone="default">mock source · {state.count} decisions</Chip>
        </div>
      </header>

      <section className="con-goal">
        <div className="con-goal-main">
          <span className="con-goal-label">Active goal</span>
          <span className="con-goal-text">Adaptive Cognitive Operating System — self-host</span>
        </div>
        <div className="con-goal-progress">
          <Bar value={0.68} tone="primary" label="68%" />
        </div>
      </section>

      <div className="con-grid con-grid--dash">
        <Panel title="Pipeline">
          <div className="con-stages">
            {overview.stage_timings.map((s) => (
              <div key={s.stage} className="con-stage">
                <span className={`con-stage-dot con-stage-dot--${s.status}`} />
                <span className="con-stage-name">{s.stage}</span>
                <span className="con-stage-ms">{fmtMs(s.ms)}</span>
              </div>
            ))}
          </div>
        </Panel>

        <Panel title="World state">
          <div className="con-world">
            <Stat label="Files" value={overview.world.files} />
            <Stat label="Memories" value={overview.world.memories} />
            <Stat label="Policies" value={overview.world.policies} />
            <Stat label="Episodes" value={overview.world.episodes} />
            <Stat label="Operators" value={overview.world.operators} />
            <Stat label="Success" value={fmtPct(overview.world.success_rate)} tone={overview.world.success_rate >= 0.85 ? 'green' : 'amber'} />
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
            {recent.map((t) => (
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
