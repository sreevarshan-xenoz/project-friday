import { useState } from 'react';
import { DecisionList } from '../console/DecisionList';
import { Panel, KeyVal, Chip, ScoreRow } from '../console/ui';
import { useConsole } from '../console/store';
import { fmtMs, reasonLabel } from '../console/format';
import './ConsoleExplanationScreen.css';

export function ConsoleExplanationScreen() {
  const { state } = useConsole();
  const [selectedId, setSelectedId] = useState<string | null>(() => state.timelines[state.timelines.length - 1]?.correlation_id ?? null);
  const t = state.timelines.find((x) => x.correlation_id === selectedId) ?? state.timelines[state.timelines.length - 1];

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">Explanation</h2>
          <p className="con-subtitle">Why FRIDAY chose what it chose</p>
        </div>
      </header>

      <div className="con-split">
        <aside className="con-split-left">
          <DecisionList timelines={state.timelines} selectedId={selectedId} onSelect={setSelectedId} />
        </aside>

        <div className="con-split-right">
          {t ? (
            <>
              <Panel title={`${t.plan.capability} → ${t.plan.operator_id}`} actions={<Chip tone="primary">explained</Chip>}>
                <p className="con-narrative">{t.explanation.narrative}</p>
                <div className="con-reasons">
                  {t.explanation.reasons.map((r) => (
                    <Chip key={r} tone="primary">{reasonLabel(r)}</Chip>
                  ))}
                </div>
              </Panel>

              <div className="con-grid">
                <Panel title="Candidate scoring">
                  <div className="con-scores">
                    {t.explanation.candidates.map((c) => (
                      <ScoreRow
                        key={c.operator_id}
                        name={`${c.operator_id}${c.selected ? ' ✓' : ''}`}
                        score={c.composite_score}
                        max={1}
                      />
                    ))}
                  </div>
                </Panel>

                <Panel title="Factors">
                  {t.explanation.experience_factor && (
                    <div className="con-factor">
                      <span className="con-factor-title">Experience</span>
                      <KeyVal k="Samples" v={t.explanation.experience_factor.sample_size} />
                      <KeyVal k="Success rate" v={`${(t.explanation.experience_factor.success_rate * 100).toFixed(1)}%`} />
                      <KeyVal k="Avg latency" v={fmtMs(t.explanation.experience_factor.average_latency_ms)} />
                      {t.explanation.experience_factor.last_failure_reason && (
                        <KeyVal k="Last failure" v={t.explanation.experience_factor.last_failure_reason} />
                      )}
                    </div>
                  )}
                  {t.explanation.reputation_factor && (
                    <div className="con-factor">
                      <span className="con-factor-title">Reputation</span>
                      <KeyVal k="Reliability" v={`${(t.explanation.reputation_factor.reliability * 100).toFixed(1)}%`} />
                      <KeyVal k="Confidence" v={`${(t.explanation.reputation_factor.confidence * 100).toFixed(0)}%`} />
                      <KeyVal k="Observations" v={t.explanation.reputation_factor.total_observations} />
                    </div>
                  )}
                  {t.explanation.calibration_factor ? (
                    <div className="con-factor">
                      <span className="con-factor-title">Calibration</span>
                      <KeyVal k="Latency offset" v={t.explanation.calibration_factor.latency_offset.toFixed(1)} />
                      <KeyVal k="Cost offset" v={t.explanation.calibration_factor.cost_offset.toFixed(3)} />
                      <KeyVal k="Success offset" v={t.explanation.calibration_factor.success_offset.toFixed(3)} />
                    </div>
                  ) : (
                    <div className="con-factor">
                      <span className="con-factor-title">Calibration</span>
                      <p className="con-empty">No learned offsets yet.</p>
                    </div>
                  )}
                  {t.explanation.policy_factors.length > 0 && (
                    <div className="con-factor">
                      <span className="con-factor-title">Policy rules</span>
                      {t.explanation.policy_factors.map((p) => (
                        <Chip key={p} tone="secondary">{p}</Chip>
                      ))}
                    </div>
                  )}
                </Panel>
              </div>
            </>
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
