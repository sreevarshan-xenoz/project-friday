import { motion } from 'framer-motion';
import { Card, SectionLabel, StatusPill } from '../components';
import { PROVIDER_OPTIONS } from '../providerCatalog';
import './ProvidersScreen.css';

/* ── Provider capability directory ─────────────────────────────────────
 * Read-only mirror of `services/action-runtime/src/adapters/provider.rs`
 * (ProviderId::default_capabilities + default_profile). The Rust registry
 * is the single source of truth; this screen is the thin-client view.
 *
 * The routing rule this table visualizes:
 *   capabilities answer "CAN it do this?"  (hard gate)
 *   scores answer    "HOW WELL does it?"   (optimization)
 *   requires > provider > score  — the precedence contract. */

interface CapabilityEntry {
  /** 14 discrete capability flags, mirroring ProviderCapabilities. */
  flags: string[];
}

interface ProviderRow {
  id: string;
  label: string;
  site: string;
  transport: 'Local' | 'Browser' | 'API';
  /** The one-line role the registry advertises. */
  role: string;
  caps: CapabilityEntry;
  /** Five scored axes: coding, realtime, deep, math, vision. */
  scores: [number, number, number, number, number];
}

const ALL_FLAGS = [
  'vision', 'deep_reasoning', 'tool_calling', 'streaming', 'citations',
  'image_generation', 'code_execution', 'local_memory', 'offline',
  'browser', 'voice', 'multimodal', 'api', 'browser_login',
] as const;

const PROVIDERS: ProviderRow[] = [
  {
    id: 'chatgpt', label: 'ChatGPT', site: 'chatgpt.com', transport: 'Browser',
    role: 'Deep reasoning',
    caps: { flags: ['vision', 'deep_reasoning', 'tool_calling', 'streaming', 'image_generation', 'multimodal', 'browser', 'browser_login'] },
    scores: [0.7, 0.2, 0.95, 0.6, 0.1],
  },
  {
    id: 'grok', label: 'Grok', site: 'grok.com', transport: 'Browser',
    role: 'Realtime',
    caps: { flags: ['vision', 'deep_reasoning', 'streaming', 'image_generation', 'browser', 'browser_login'] },
    scores: [0.5, 0.9, 0.4, 0.3, 0.1],
  },
  {
    id: 'gemini', label: 'Gemini', site: 'gemini.google.com', transport: 'Browser',
    role: 'Vision',
    caps: { flags: ['vision', 'deep_reasoning', 'tool_calling', 'streaming', 'image_generation', 'multimodal', 'browser', 'browser_login'] },
    scores: [0.6, 0.5, 0.8, 0.7, 0.9],
  },
  {
    id: 'claude', label: 'Claude', site: 'claude.ai', transport: 'Browser',
    role: 'Coding + citations',
    caps: { flags: ['vision', 'deep_reasoning', 'tool_calling', 'streaming', 'code_execution', 'citations', 'browser', 'browser_login'] },
    scores: [0.9, 0.2, 0.9, 0.4, 0.2],
  },
  {
    id: 'perplexity', label: 'Perplexity', site: 'perplexity.ai', transport: 'Browser',
    role: 'Realtime + citations',
    caps: { flags: ['citations', 'streaming', 'tool_calling', 'deep_reasoning', 'browser', 'browser_login'] },
    scores: [0.4, 0.9, 0.6, 0.3, 0.2],
  },
  {
    id: 'local', label: 'Local · Gemma', site: 'localhost:8080', transport: 'Local',
    role: 'On-machine, private',
    caps: { flags: ['offline', 'local_memory', 'api', 'streaming'] },
    scores: [0.8, 0.0, 0.6, 0.7, 0.1],
  },
];

const AXES = ['Coding', 'Realtime', 'Deep', 'Math', 'Vision'] as const;

export function ProvidersScreen() {
  return (
    <div className="providers">
      <motion.div
        className="providers-head"
        initial={{ opacity: 0, y: -10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
      >
        <h2 className="providers-title">Provider Registry</h2>
        <p className="providers-sub">
          The capability directory the runtime routes against — <em>can it?</em> gates, then{' '}
          <em>how well?</em> ranks. Read-only view of the Rust registry.
        </p>

        <div className="providers-rule">
          <div className="providers-rule-item">
            <span className="providers-rule-n">1</span>
            <div>
              <strong>requires</strong> — hard constraint (exclusion before ranking)
            </div>
          </div>
          <div className="providers-rule-item">
            <span className="providers-rule-n">2</span>
            <div>
              <strong>provider</strong> — preferred target (failover proceeds after)
            </div>
          </div>
          <div className="providers-rule-item">
            <span className="providers-rule-n">3</span>
            <div>
              <strong>score</strong> — optimization (orders the survivors)
            </div>
          </div>
        </div>
      </motion.div>

      <div className="providers-list">
        {PROVIDERS.map((p, i) => (
          <motion.div
            key={p.id}
            initial={{ opacity: 0, y: 14 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.4, delay: i * 0.06, ease: [0.22, 1, 0.36, 1] }}
          >
            <Card className="providers-card">
              <div className="providers-card-head">
                <div className="providers-card-id">
                  <span className="providers-card-mark">◆</span>
                  <div>
                    <div className="providers-card-name">{p.label}</div>
                    <div className="providers-card-site">{p.site}</div>
                  </div>
                </div>
                <div className="providers-card-right">
                  <StatusPill
                    level={p.transport === 'Local' ? 'green' : 'amber'}
                    text={p.transport}
                    size="sm"
                  />
                  <span className="providers-card-role">{p.role}</span>
                </div>
              </div>

              <div className="providers-card-body">
                <div className="providers-caps">
                  <SectionLabel>Capabilities (can it?)</SectionLabel>
                  <div className="providers-caps-list">
                    {ALL_FLAGS.map((f) => (
                      <span
                        key={f}
                        className={`providers-cap ${p.caps.flags.includes(f) ? 'providers-cap--on' : ''}`}
                      >
                        {p.caps.flags.includes(f) ? '✓' : '·'} {f}
                      </span>
                    ))}
                  </div>
                </div>

                <div className="providers-scores">
                  <SectionLabel>Expert profile (how well?)</SectionLabel>
                  <div className="providers-scores-list">
                    {AXES.map((axis, j) => (
                      <div key={axis} className="providers-score">
                        <span className="providers-score-axis">{axis}</span>
                        <div className="providers-score-track">
                          <div
                            className="providers-score-fill"
                            style={{ width: `${Math.round(p.scores[j] * 100)}%` }}
                          />
                        </div>
                        <span className="providers-score-val">{p.scores[j].toFixed(1)}</span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </Card>
          </motion.div>
        ))}
      </div>

      <div className="providers-foot">
        <SectionLabel>Routing intent in the composer</SectionLabel>
        <div className="providers-foot-row">
          {PROVIDER_OPTIONS.filter((o) => o.id !== 'auto').map((o) => (
            <span key={o.id} className="providers-foot-chip">
              {o.label}
              <span className="providers-foot-t">{o.transport}</span>
            </span>
          ))}
        </div>
        <p className="providers-foot-note">
          The dashboard never calls a provider directly — it sets routing intent; the runtime
          (ExpertRouter / InferenceFabric) picks the expert and the transport.
        </p>
      </div>
    </div>
  );
}
