import { Fragment } from 'react';
import { motion } from 'framer-motion';
import { useConsole } from '../console/store';
import { Chip } from '../console/ui';
import type {
  ExecutivePhaseState,
  InformationRequirement,
  InvestigationChannel,
  InvestigationGraphDTO,
  PhaseStatus,
} from '../console/types';
import './ExecutionGraphScreen.css';

/** Channel identity for the branch cards — icons + human labels. */
const CHANNEL_META: Record<InvestigationChannel, { icon: string; label: string }> = {
  web: { icon: '◎', label: 'Web' },
  docs: { icon: '▤', label: 'Docs' },
  codeintel: { icon: '⌬', label: 'Code Intelligence' },
  memory: { icon: '❖', label: 'Memory' },
  git: { icon: '⛃', label: 'Git' },
  filesystem: { icon: '▣', label: 'Filesystem' },
};

/** The eleven-phase executive cycle (mirrors `services/goal-execution`). */
const EXEC_ORDER: string[] = [
  'Observe',
  'Attend',
  'Investigate',
  'Plan',
  'Simulate',
  'Execute',
  'Verify',
  'Reflect',
  'Consolidate',
  'Learn',
  'Idle',
];

/**
 * Which executive-cycle window each graph phase lives in. `investigate`
 * occupies the real Investigate phase (the M7 evidence gate), `plan`
 * occupies Plan→Simulate, `execute` occupies Execute→Idle.
 */
const PHASE_WINDOWS: Record<string, [number, number]> = {
  investigate: [2, 2],
  plan: [3, 4],
  execute: [5, 10],
};

/** Derive a graph phase's live status from the executive cycle. */
function statusFor(phase: string, exec: ExecutivePhaseState[]): PhaseStatus {
  const runningName = exec.find((p) => p.status === 'running')?.name;
  const cur = runningName ? EXEC_ORDER.indexOf(runningName) : -1;
  if (cur < 0) return 'waiting';
  const [lo, hi] = PHASE_WINDOWS[phase] ?? [5, 10];
  if (cur < lo) return 'waiting';
  if (cur <= hi) return 'running';
  return 'done';
}

function LiveBadge({ connected, fps }: { connected: boolean; fps: number }) {
  return (
    <span className={`con-graph-live${connected ? ' con-graph-live--on' : ''}`}>
      <span className="con-graph-live-dot" />
      LIVE{fps > 0 ? ` · ${fps} fps` : ''}
    </span>
  );
}

function BranchCard({ req, index }: { req: InformationRequirement; index: number }) {
  const meta = CHANNEL_META[req.channel];
  const pct = Math.round(req.confidence * 100);
  return (
    <motion.div
      className={`con-graph-branch con-graph-branch--${req.channel}`}
      initial={{ opacity: 0, y: 14, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ delay: 0.3 + index * 0.14, duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
    >
      <div className="con-graph-branch-head">
        <span className="con-graph-branch-icon">{meta.icon}</span>
        <span className="con-graph-branch-name">{meta.label}</span>
        <span className="con-graph-branch-pct">{pct}%</span>
      </div>
      <div className="con-graph-bar">
        <motion.div
          className={`con-graph-bar-fill con-graph-bar-fill--${req.channel}`}
          initial={{ width: 0 }}
          animate={{ width: `${pct}%` }}
          transition={{ delay: 0.45 + index * 0.14, duration: 0.7, ease: 'easeOut' }}
        />
      </div>
      <div className="con-graph-branch-rationale">matched · {req.rationale}</div>
    </motion.div>
  );
}

function PhaseRail({ graph, exec }: { graph: InvestigationGraphDTO; exec: ExecutivePhaseState[] }) {
  return (
    <div className="con-graph-phases">
      {graph.phases.map((phase, i) => {
        const status = statusFor(phase, exec);
        return (
          <Fragment key={phase}>
            {i > 0 && <motion.div className="con-graph-phase-link" layout />}
            <motion.div
              className={`con-graph-phase con-graph-phase--${status}`}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.5 + i * 0.12, duration: 0.35 }}
            >
              <span className="con-graph-phase-glyph">
                {status === 'done' ? '✓' : status === 'running' ? '●' : '○'}
              </span>
              <span className="con-graph-phase-name">{phase}</span>
              {status === 'running' && <span className="con-graph-phase-pulse" />}
            </motion.div>
          </Fragment>
        );
      })}
    </div>
  );
}

export function ExecutionGraphScreen() {
  const { status, error, dashboard, runtime } = useConsole();
  const graph = dashboard.graph;

  return (
    <div className="con-screen">
      <header className="con-head">
        <div>
          <h2 className="con-title">Execution Graph</h2>
          <p className="con-subtitle">
            Goal → evidence branches → phases · what FRIDAY decided it needs to know before acting
          </p>
        </div>
        <div className="con-head-right">
          <LiveBadge connected={runtime.connected} fps={runtime.fps} />
        </div>
      </header>

      {status !== 'ready' || !graph ? (
        <p className="con-empty">{status === 'error' ? error : 'Waiting for the first trace…'}</p>
      ) : (
        <motion.div
          key={graph.goal}
          className="con-graph-flow"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.35 }}
        >
          {/* Goal node */}
          <motion.div
            className="con-graph-goal"
            initial={{ opacity: 0, scale: 0.94 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
          >
            <span className="con-graph-goal-tag">GOAL</span>
            <span className="con-graph-goal-text">{graph.goal}</span>
          </motion.div>

          <div className="con-graph-link" />

          {/* Evidence branches */}
          {graph.requirements.length > 0 ? (
            <div className="con-graph-branches">
              {graph.requirements.map((req, i) => (
                <BranchCard key={req.channel} req={req} index={i} />
              ))}
            </div>
          ) : (
            <motion.div
              className="con-graph-selfcontained"
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.3, duration: 0.4 }}
            >
              <span className="con-graph-selfcontained-icon">◎</span>
              <span>No investigation required — the goal is self-contained</span>
            </motion.div>
          )}

          <div className="con-graph-link" />

          {/* Phase chain */}
          <PhaseRail graph={graph} exec={dashboard.executive} />

          <div className="con-graph-foot">
            <Chip tone="default">phases {graph.phases.join(' → ')}</Chip>
            <Chip tone="default">
              {graph.requirements.length} evidence branch{graph.requirements.length === 1 ? '' : 'es'}
            </Chip>
          </div>
        </motion.div>
      )}
    </div>
  );
}
