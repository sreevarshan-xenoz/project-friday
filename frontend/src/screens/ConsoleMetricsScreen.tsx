import { Panel, Stat, Chip } from '../console/ui';
import { useConsole } from '../console/store';
import { fmtPct, fmtMs, fmtCost } from '../console/format';
import './ConsoleMetricsScreen.css';

export function ConsoleMetricsScreen() {
  const { status, error, metrics: m } = useConsole();

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">Metrics</h2>
          <p className="con-subtitle">Derived read models · {m.decisions} decisions</p>
        </div>
        <Chip tone="default">stateless derivation</Chip>
      </header>

      {status !== 'ready' ? (
        <p className="con-empty">{status === 'error' ? error : 'Loading read models…'}</p>
      ) : (
      <div className="con-grid con-grid--metrics">
        <Panel title="Prediction accuracy">
          <div className="con-metric-stats">
            <Stat label="Accuracy" value={fmtPct(m.prediction.accuracy)} tone={m.prediction.accuracy >= 0.8 ? 'green' : 'amber'} />
            <Stat label="Correct / total" value={`${m.prediction.correct_predictions} / ${m.prediction.decisions}`} />
            <Stat label="Mean latency error" value={fmtMs(m.prediction.mean_latency_error_ms)} />
            <Stat label="Mean cost error" value={fmtCost(m.prediction.mean_cost_error)} />
            <Stat label="Success bias" value={m.prediction.mean_success_bias.toFixed(3)} tone={Math.abs(m.prediction.mean_success_bias) < 0.1 ? 'green' : 'amber'} />
          </div>
        </Panel>

        <Panel title="Calibration">
          <div className="con-metric-stats">
            <Stat label="Decisions w/ offsets" value={m.calibration.decisions_with_calibration} />
            <Stat label="Mean latency offset" value={`${m.calibration.mean_latency_offset.toFixed(1)} ms`} />
            <Stat label="Mean cost offset" value={m.calibration.mean_cost_offset.toFixed(3)} />
            <Stat label="Mean success offset" value={m.calibration.mean_success_offset.toFixed(3)} />
            <Stat
              label="Converging"
              value={<Chip tone={m.calibration.converging ? 'green' : 'amber'}>{m.calibration.converging ? 'yes' : 'no'}</Chip>}
            />
          </div>
        </Panel>

        <Panel title="Fallback">
          <div className="con-metric-stats">
            <Stat label="Fallback rate" value={fmtPct(m.fallback.fallback_rate)} />
            <Stat label="Avg fallback depth" value={m.fallback.avg_fallback_depth.toFixed(2)} />
            <Stat label="Chains" value={m.fallback.decisions_with_fallback_chain} />
            <Stat label="Failures" value={m.fallback.failures} tone={m.fallback.failures ? 'amber' : 'green'} />
            <Stat label="Failure rate" value={fmtPct(m.fallback.failure_rate)} tone={m.fallback.failure_rate > 0.2 ? 'red' : 'green'} />
          </div>
        </Panel>

        <Panel title="Reputation">
          <div className="con-metric-stats">
            <Stat label="Coverage" value={fmtPct(m.reputation.coverage)} />
            <Stat label="Avg reliability" value={fmtPct(m.reputation.avg_reliability)} tone={m.reputation.avg_reliability >= 0.8 ? 'green' : 'amber'} />
            <Stat label="Avg confidence" value={fmtPct(m.reputation.avg_confidence)} />
            <Stat label="Observations" value={m.reputation.total_observations} />
            <Stat label="Distinct operators" value={m.reputation.distinct_operators} />
          </div>
        </Panel>

        <Panel title="Experience">
          <div className="con-metric-stats">
            <Stat label="Coverage" value={fmtPct(m.experience.coverage)} />
            <Stat label="Distinct pairs" value={m.experience.distinct_pairs} />
            <Stat label="Total samples" value={m.experience.total_samples} />
            <Stat label="Avg sample size" value={m.experience.avg_sample_size.toFixed(1)} />
            <Stat label="Avg success rate" value={fmtPct(m.experience.avg_success_rate)} />
            <Stat label="Avg latency" value={fmtMs(m.experience.avg_latency_ms)} />
          </div>
        </Panel>
      </div>
      )}
    </div>
  );
}
