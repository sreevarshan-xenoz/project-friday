import { useState } from 'react';
import { DecisionList } from '../console/DecisionList';
import { Panel, KeyVal, Chip } from '../console/ui';
import { useConsole } from '../console/store';
import { fmtMs, fmtCost, fmtSigned, fmtDateTime, reasonLabel } from '../console/format';
import type { DecisionTimeline } from '../console/types';
import './ConsoleTimelineScreen.css';

function PredictionRow({ t }: { t: DecisionTimeline }) {
  const estLat = t.prediction.estimated_latency_ms;
  const actLat = t.outcome.latency_ms;
  const estCost = t.prediction.estimated_cost;
  const actCost = t.outcome.cost;
  const latTone = Math.abs(estLat - actLat) / Math.max(estLat, 1) > 0.5 ? 'red' : 'green';
  return (
    <div className="con-compare">
      <KeyVal k="Estimated latency" v={`${fmtMs(estLat)} (${fmtSigned(estLat - actLat)} actual)`} />
      <KeyVal k="Actual latency" v={fmtMs(actLat)} />
      <KeyVal k="Estimated cost" v={`${fmtCost(estCost)}`} />
      <KeyVal k="Actual cost" v={fmtCost(actCost)} />
      <KeyVal
        k="Success probability"
        v={`${(t.prediction.estimated_success_probability * 100).toFixed(1)}% → ${t.outcome.success ? 'success' : 'failure'}`}
      />
      <KeyVal
        k="Latency error"
        v={<Chip tone={latTone}>{fmtSigned(t.feedback.latency_error_ms, 0)} ms</Chip>}
      />
    </div>
  );
}

export function ConsoleTimelineScreen() {
  const { state } = useConsole();
  const [selectedId, setSelectedId] = useState<string | null>(() => state.timelines[state.timelines.length - 1]?.correlation_id ?? null);
  const t = state.timelines.find((x) => x.correlation_id === selectedId) ?? state.timelines[state.timelines.length - 1];

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">Timeline</h2>
          <p className="con-subtitle">Immutable decision reconstruction · {state.count} records</p>
        </div>
      </header>

      <div className="con-split">
        <aside className="con-split-left">
          <DecisionList timelines={state.timelines} selectedId={selectedId} onSelect={setSelectedId} />
        </aside>

        <div className="con-split-right">
          {t ? (
            <Panel
              title={`${t.plan.capability} · ${fmtDateTime(t.record.timestamp_ms)}`}
              actions={
                <div className="con-panel-actions">
                  <Chip tone={t.outcome.success ? 'green' : 'red'}>{t.outcome.success ? 'success' : 'failed'}</Chip>
                  <Chip tone="default">plan v{t.plan.version}</Chip>
                </div>
              }
            >
              <div className="con-tl">
                <KeyVal k="Correlation ID" v={t.correlation_id} />
                <KeyVal k="Plan ID" v={t.plan.plan_id} />
                <KeyVal k="Operator" v={t.plan.operator_id} />
                <KeyVal k="Prediction confidence" v={`${(t.prediction.confidence * 100).toFixed(0)}%`} />
                <KeyVal k="Fallback chain" v={t.plan.fallback_chain.length ? `${t.plan.fallback_chain.map((f) => f.operator_id).join(' → ')}` : 'none'} />
              </div>

              <h4 className="con-tl-sub">Prediction vs outcome</h4>
              <PredictionRow t={t} />

              <h4 className="con-tl-sub">Feedback</h4>
              <div className="con-fb">
                <KeyVal k="Success bias" v={fmtSigned(t.feedback.success_bias)} />
                <KeyVal k="Cost error" v={fmtSigned(t.feedback.cost_error)} />
                <KeyVal k="Confidence adjustment" v={fmtSigned(t.feedback.confidence_adjustment)} />
                <KeyVal
                  k="Model update"
                  v={<Chip tone={t.feedback.trigger_model_update ? 'amber' : 'green'}>{t.feedback.trigger_model_update ? 'triggered' : 'none'}</Chip>}
                />
              </div>

              <h4 className="con-tl-sub">Explanation</h4>
              <div className="con-reasons">
                {t.explanation.reasons.map((r) => (
                  <Chip key={r} tone="primary">{reasonLabel(r)}</Chip>
                ))}
              </div>
              <p className="con-narrative">{t.explanation.narrative}</p>
            </Panel>
          ) : (
            <Panel title="No decision selected">
              <p className="con-empty">Select a decision from the list.</p>
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}
