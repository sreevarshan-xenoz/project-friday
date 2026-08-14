import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useFriday } from '../hooks/useFriday';
import { useConnection, useEvents, useCommandResults } from '../hooks/useProtocol';
import type { EvidenceChannel, GoalResult, ParsedEvent } from '../protocol/protocol';
import { Methods } from '../protocol/protocol';
import { PROVIDER_OPTIONS, providerById } from '../providerCatalog';
import { arrayLines, classifyResult, entryCountLabel, fileListLines, structureRows } from './resultPresentation';
import type { ResultView } from './resultPresentation';
import './ChatScreen.css';

/* ── Conversation model (client-side, thin-client persistence) ───────── */
export interface ChatMessage {
  id: string;
  role: 'user' | 'friday';
  kind: 'text' | 'goal';
  text: string;
  /** The goal text for goal-kind messages (echoed by the runtime). */
  goal?: string;
  /** The runtime goal uuid (`goal.started.id`) — the run's real identity, so
   * repeated identical goals are visibly distinct. */
  goalId?: string;
  /** Lifecycle stage of an in-flight goal message. */
  stage?: 'sent' | 'understood' | 'started' | 'done' | 'failed' | 'cancelled' | 'paused';
  phases?: number;
  evidenceVerified?: number;
  /** Per-channel evidence lifecycle (`goal.completed.channels`). */
  channels?: EvidenceChannel[];
  /** The goal's actual result — the execution output (`goal.completed.result`). */
  result?: GoalResult;
  /** Investigation strategy that produced the plan (`llm` | `heuristic`). */
  strategy?: string;
  /** The provider that served the model answer (`goal.completed.provider`). */
  provider?: string;
  /** The model id that answered (`goal.completed.model`). */
  model?: string;
  error?: string;
  /** Whether this goal's episode was held for a human and the card offers an
   * approve action (`goal.completed.pending_approval`). */
  pendingApproval?: boolean;
  /** The held episode's id — the `task_id` a `task.approve` must target. */
  pendingApprovalId?: string;
  /** Set once `task.approve` → `goal.approved` admitted the held episode. */
  approved?: boolean;
  /** Routing intent chosen in the composer (auto = runtime decides). */
  providerIntent?: string;
  /** command id so the accepted ack can resolve the stage. */
  cmdId?: string;
  ts: number;
}

export interface Conversation {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: ChatMessage[];
}

const STORE_KEY = 'friday.chat.conversations';
const ACTIVE_KEY = 'friday.chat.active';

function loadConversations(): Conversation[] {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Conversation[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function loadActive(): string | null {
  return localStorage.getItem(ACTIVE_KEY);
}

const EASE = [0.22, 1, 0.36, 1] as const;

export function ChatScreen() {
  const client = useFriday();
  const conn = useConnection();
  const events = useEvents(100);
  const results = useCommandResults();

  const [conversations, setConversations] = useState<Conversation[]>(loadConversations);
  const [activeId, setActiveId] = useState<string | null>(loadActive);
  const [input, setInput] = useState('');
  const [model, setModel] = useState('auto');
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [askMenuFor, setAskMenuFor] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);

  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  /** Dedupe guard: `input` is read from a closure, so a second Enter/keypress
   * before React re-renders (empty input) would re-send the same goal. */
  const lastSentRef = useRef<{ text: string; at: number } | null>(null);

  const active = useMemo(
    () => conversations.find((c) => c.id === activeId) ?? null,
    [conversations, activeId],
  );

  /* Persist on change */
  useEffect(() => {
    localStorage.setItem(STORE_KEY, JSON.stringify(conversations));
  }, [conversations]);
  useEffect(() => {
    if (activeId) localStorage.setItem(ACTIVE_KEY, activeId);
  }, [activeId]);

  /* Autoscroll on new messages */
  const msgCount = active?.messages.length ?? 0;
  const lastStage = active?.messages[active.messages.length - 1]?.stage;
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [msgCount, lastStage]);

  /* Resolve in-flight goal messages from the lifecycle event stream.
   * plan.goal (understood) → goal.started (started) → goal.completed/failed. */
  useEffect(() => {
    if (!events.length) return;
    setConversations((prev) => {
      let changed = false;
      const next = prev.map((conv) => {
        const working = conv.messages.find(
          (m) => m.role === 'friday' && m.kind === 'goal' && (m.stage === 'sent' || m.stage === 'understood' || m.stage === 'started'),
        );
        if (!working) return conv;
        // Newest-first stream + a timestamp guard: only events published AFTER
        // this message was sent can advance it, so re-sending the same goal
        // text never resolves the new card against the old goal's events.
        const evt = events.find(
          (e) =>
            e.timestamp >= working.ts &&
            (e.parsed.task?.title === working.goal || e.parsed.summary === working.goal),
        );
        if (!evt) return conv;
        const p = evt.parsed;
        const stage: ChatMessage['stage'] =
          p.kind === 'goal.started'
            ? 'started'
            : p.kind === 'goal.completed'
              ? 'done'
              : p.kind === 'goal.failed'
                ? 'failed'
                : p.kind === 'goal.cancelled'
                  ? 'cancelled'
                  : p.kind === 'goal.paused'
                    ? 'paused'
                    : working.stage;
        if (stage === working.stage) return conv;
        changed = true;
        const updated: ChatMessage = {
          ...working,
          stage,
          goalId: p.goalId ?? working.goalId,
          phases: p.phases ?? working.phases,
          evidenceVerified: p.evidenceVerified ?? working.evidenceVerified,
          channels: p.channels ?? working.channels,
          strategy: p.strategy ?? working.strategy,
          provider: p.provider ?? working.provider,
          model: p.model ?? working.model,
          result: p.result ?? working.result,
          error: p.error ?? working.error,
          pendingApproval: p.pendingApproval ?? working.pendingApproval,
          pendingApprovalId: p.pendingApprovalId ?? working.pendingApprovalId,
          text:
            stage === 'done'
              ? hasRenderableResult(p)
                ? ''
                : completedFallback(p)
              : stage === 'failed'
                ? p.error ?? 'The goal could not be completed.'
                : stage === 'cancelled'
                  ? 'The run was cancelled.'
                  : stage === 'paused'
                    ? 'The run was paused.'
                    : stage === 'started'
                      ? 'FRIDAY is working…'
                      : 'FRIDAY understood the request.',
        };
        return { ...conv, messages: conv.messages.map((m) => (m.id === working.id ? updated : m)), updatedAt: Date.now() };
      });
      return changed ? next : prev;
    });
  }, [events]);

  /* Mark a goal as understood once its command is accepted. */
  useEffect(() => {
    if (!results.length) return;
    setConversations((prev) => {
      let changed = false;
      const next = prev.map((conv) => ({
        ...conv,
        messages: conv.messages.map((m) => {
          if (m.role !== 'friday' || m.cmdId == null || m.stage !== 'sent') return m;
          const r = results.find((x) => x.id === m.cmdId);
          if (!r || !r.ok) return m;
          changed = true;
          // Annotated so TS keeps `stage` narrow despite the early `return m;`
          // above (an inline literal there would widen `stage` to string).
          const understood: ChatMessage = {
            ...m,
            stage: 'understood',
            text: 'FRIDAY understood the request.',
          };
          return understood;
        }),
      }));
      return changed ? next : prev;
    });
  }, [results]);

  /* Admit a held episode: `task.approve` → `goal.approved` carries the held
   * episode's id as `task_id` — matched exactly against the card's
   * `pendingApprovalId`, so the approve resolves THIS card's state
   * transition, never another run's. */
  useEffect(() => {
    if (!events.length) return;
    setConversations((prev) => {
      let changed = false;
      const next = prev.map((conv) => {
        const held = conv.messages.find(
          (m) => m.role === 'friday' && m.kind === 'goal' && !!m.pendingApprovalId && !m.approved,
        );
        if (!held) return conv;
        const evt = events.find(
          (e) => e.parsed.kind === 'goal.approved' && e.parsed.taskId === held.pendingApprovalId,
        );
        if (!evt) return conv;
        changed = true;
        const updated: ChatMessage = {
          ...held,
          approved: true,
          text: 'Approved — the held episode was admitted to memory.',
        };
        return {
          ...conv,
          messages: conv.messages.map((m) => (m.id === held.id ? updated : m)),
          updatedAt: Date.now(),
        };
      });
      return changed ? next : prev;
    });
  }, [events]);

  const deleteConversation = useCallback((id: string) => {
    setConversations((prev) => prev.filter((c) => c.id !== id));
    setActiveId((cur) => (cur === id ? null : cur));
  }, []);

  const startNew = useCallback(() => {
    const conv: Conversation = {
      id: `conv-${Date.now()}`,
      title: 'New conversation',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      messages: [],
    };
    setConversations((prev) => [conv, ...prev]);
    setActiveId(conv.id);
    setDetailsOpen(false);
  }, []);

  const sendGoal = useCallback(
    (text: string, intent: string) => {
      const goal = text.trim();
      if (!goal) return;
      // Ignore an identical resubmit inside a short window — a held Enter or a
      // fast double-click must not create duplicate goal runs (the reviewer's
      // "same card three times" screenshot is exactly this pattern).
      const now = Date.now();
      if (lastSentRef.current && lastSentRef.current.text === goal && now - lastSentRef.current.at < 1500) {
        return;
      }
      lastSentRef.current = { text: goal, at: now };
      const cmdId = client.sendGoal(goal);
      const ts = Date.now();
      const userMsg: ChatMessage = { id: `u-${ts}`, role: 'user', kind: 'text', text: goal, ts };
      const fridayMsg: ChatMessage = {
        id: `f-${ts}`,
        role: 'friday',
        kind: 'goal',
        text: 'FRIDAY is working…',
        goal,
        stage: 'sent',
        providerIntent: intent,
        cmdId,
        ts: ts + 1,
      };
      if (active) {
        // Append to the open conversation.
        const title = active.title === 'New conversation' ? goal.slice(0, 42) : active.title;
        setConversations((prev) =>
          prev.map((c) =>
            c.id === active.id
              ? { ...c, title, updatedAt: ts, messages: [...c.messages, userMsg, fridayMsg] }
              : c,
          ),
        );
      } else {
        // No open conversation — create one holding both messages, in ONE
        // setState so the append can never race the create.
        const conv: Conversation = {
          id: `conv-${Date.now()}`,
          title: goal.slice(0, 42),
          createdAt: ts,
          updatedAt: ts,
          messages: [userMsg, fridayMsg],
        };
        setConversations((prev) => [conv, ...prev]);
        setActiveId(conv.id);
      }
      setInput('');
      if (textareaRef.current) textareaRef.current.style.height = 'auto';
    },
    [client, active],
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    sendGoal(input, model);
  };

  const regenerate = useCallback(
    (msg: ChatMessage) => {
      if (msg.goal) sendGoal(msg.goal, msg.providerIntent ?? 'auto');
    },
    [sendGoal],
  );

  const askAnotherModel = useCallback(
    (msg: ChatMessage, intent: string) => {
      setAskMenuFor(null);
      if (msg.goal) sendGoal(msg.goal, intent);
    },
    [sendGoal],
  );

  const copyText = useCallback((text: string) => {
    void navigator.clipboard?.writeText(text).catch(() => {});
  }, []);

  /* S3 human control: a running goal can be cancelled or paused by its run id
   * (`goal.started.id`). The runtime turns the command into the real
   * `goal.cancelled`/`goal.paused` transition the card resolves to. */
  const controlGoal = useCallback(
    (msg: ChatMessage, method: string) => {
      if (!msg.goalId) return;
      client.sendCommand(method, { task_id: msg.goalId });
    },
    [client],
  );

  /* S3 approval: approve the exact held episode this card surfaced. */
  const approveGoal = useCallback(
    (msg: ChatMessage) => {
      if (!msg.pendingApprovalId) return;
      client.sendCommand(Methods.TASK_APPROVE, { task_id: msg.pendingApprovalId });
    },
    [client],
  );

  const grouped = useMemo(() => {
    const groups: { label: string; items: Conversation[] }[] = [];
    const now = new Date();
    const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const startOfYesterday = startOfToday - 86_400_000;
    const today = conversations.filter((c) => c.updatedAt >= startOfToday);
    const yesterday = conversations.filter((c) => c.updatedAt >= startOfYesterday && c.updatedAt < startOfToday);
    const earlier = conversations.filter((c) => c.updatedAt < startOfYesterday);
    if (today.length) groups.push({ label: 'Today', items: today });
    if (yesterday.length) groups.push({ label: 'Yesterday', items: yesterday });
    if (earlier.length) groups.push({ label: 'Earlier', items: earlier });
    return groups;
  }, [conversations]);

  const lastGoal = useMemo(
    () =>
      [...(active?.messages ?? [])]
        .reverse()
        .find((m) => m.role === 'friday' && m.kind === 'goal'),
    [active?.messages],
  );
  // failed caps at 3 so the drawer's "Verified result" step stays unchecked.
  // cancelled/paused track by phases completed (verify is always skipped).
  const stageIdx = lastGoal ? stageToIdx(lastGoal) : 0;

  const modelOption = providerById(model);

  return (
    <div className="chat-dash">
      {/* ── Conversation rail ─────────────────────────────────── */}
      <aside className="chat-rail">
        <button className="chat-new" onClick={startNew}>
          <span className="chat-new-plus">+</span>
          New chat
        </button>

        <div className="chat-rail-scroll">
          {grouped.map((g) => (
            <div key={g.label} className="chat-rail-group">
              <span className="chat-rail-label">{g.label}</span>
              {g.items.map((c) => (
                <div
                  key={c.id}
                  role="button"
                  tabIndex={0}
                  className={`chat-rail-item ${c.id === activeId ? 'chat-rail-item--active' : ''}`}
                  onClick={() => setActiveId(c.id)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') setActiveId(c.id);
                  }}
                >
                  <span className="chat-rail-item-dot">◆</span>
                  <span className="chat-rail-item-title">{c.title}</span>
                  <button
                    className="chat-rail-del"
                    title="Delete conversation"
                    onClick={(e) => {
                      e.stopPropagation();
                      deleteConversation(c.id);
                    }}
                  >
                    ×
                  </button>
                </div>
              ))}
            </div>
          ))}
          {conversations.length === 0 && (
            <div className="chat-rail-empty">No conversations yet.</div>
          )}
        </div>

        <div className="chat-rail-foot">
          <span className={`chat-rail-dot ${conn.status === 'connected' ? 'chat-rail-dot--on' : ''}`} />
          {conn.status === 'connected' ? 'Runtime connected' : conn.status}
        </div>
      </aside>

      {/* ── Thread ───────────────────────────────────────────── */}
      <section className="chat-main">
        <header className="chat-thread-header">
          <div className="chat-thread-id">
            <span className="chat-thread-mark">◆</span>
            <div>
              <div className="chat-thread-title">{active?.title ?? 'FRIDAY'}</div>
              <div className="chat-thread-sub">
                {active ? `${active.messages.filter((m) => m.role === 'user').length} prompts` : 'Cognitive runtime'}
              </div>
            </div>
          </div>
          <button
            className={`chat-details-toggle ${detailsOpen ? 'chat-details-toggle--on' : ''}`}
            onClick={() => setDetailsOpen((v) => !v)}
          >
            Details
          </button>
        </header>

        <div className="chat-thread">
          {!active || active.messages.length === 0 ? (
            <EmptyState onPick={(g) => sendGoal(g, model)} />
          ) : (
            <AnimatePresence initial={false}>
              {active.messages.map((msg) =>
                msg.role === 'user' ? (
                  <motion.div
                    key={msg.id}
                    className="chat-row chat-row--user"
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.25, ease: EASE }}
                  >
                    <div className="chat-bubble chat-bubble--user">{msg.text}</div>
                  </motion.div>
                ) : msg.kind === 'goal' ? (
                  <GoalCard
                    key={msg.id}
                    msg={msg}
                    onCopy={() => copyText(msg.goal ?? '')}
                    onRegenerate={() => regenerate(msg)}
                    onAsk={() => setAskMenuFor(msg.id)}
                    askOpen={askMenuFor === msg.id}
                    onCloseAsk={() => setAskMenuFor(null)}
                    onPickAsk={(intent) => askAnotherModel(msg, intent)}
                    onCancel={() => controlGoal(msg, Methods.TASK_CANCEL)}
                    onPause={() => controlGoal(msg, Methods.TASK_PAUSE)}
                    onApprove={() => approveGoal(msg)}
                  />
                ) : (
                  <motion.div
                    key={msg.id}
                    className="chat-row chat-row--friday"
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.25, ease: EASE }}
                  >
                    <div className="chat-bubble chat-bubble--friday">{msg.text}</div>
                  </motion.div>
                ),
              )}
            </AnimatePresence>
          )}
          <div ref={bottomRef} />
        </div>

        {/* ── Composer + status strip ─────────────────────────── */}
        <div className="chat-composer-wrap">
          <div className="chat-status-strip">
            <span className="chat-status-model">
              <span className="chat-status-emoji">🧠</span>
              {modelOption.label}
              <span className="chat-status-transport">· {modelOption.transport}</span>
            </span>
            <span className="chat-status-meta">
              <span className="chat-status-meta-item">runtime-routed</span>
              <span className="chat-status-meta-item">
                {conn.health ? `${conn.health.latency_ms}ms` : '—'}
              </span>
            </span>
          </div>

          <form className="chat-composer" onSubmit={handleSubmit}>
            <textarea
              ref={textareaRef}
              className="chat-composer-input"
              rows={1}
              value={input}
              placeholder="Ask FRIDAY anything…"
              onChange={(e) => {
                setInput(e.target.value);
                e.target.style.height = 'auto';
                e.target.style.height = `${Math.min(e.target.scrollHeight, 160)}px`;
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  handleSubmit(e);
                }
              }}
            />
            <div className="chat-composer-bar">
              <div className="chat-composer-tools">
                <button type="button" className="chat-tool" title="Attach (coming soon)" disabled>
                  📎
                </button>
                <button type="button" className="chat-tool" title="Voice (coming soon)" disabled>
                  🎤
                </button>
              </div>

              <div className="chat-composer-right">
                <div className="chat-model">
                  <button
                    type="button"
                    className="chat-model-btn"
                    onClick={() => setModelMenuOpen((v) => !v)}
                  >
                    <span className="chat-model-label">{modelOption.label}</span>
                    <span className="chat-model-caret">▾</span>
                  </button>
                  <AnimatePresence>
                    {modelMenuOpen && (
                      <>
                        <div className="chat-model-backdrop" onClick={() => setModelMenuOpen(false)} />
                        <motion.div
                          className="chat-model-menu"
                          initial={{ opacity: 0, y: -6, scale: 0.98 }}
                          animate={{ opacity: 1, y: 0, scale: 1 }}
                          exit={{ opacity: 0, y: -6, scale: 0.98 }}
                          transition={{ duration: 0.15, ease: EASE }}
                        >
                          {PROVIDER_OPTIONS.map((p) => (
                            <button
                              key={p.id}
                              className={`chat-model-option ${p.id === model ? 'chat-model-option--active' : ''}`}
                              onClick={() => {
                                setModel(p.id);
                                setModelMenuOpen(false);
                              }}
                            >
                              <span className="chat-model-option-top">
                                <span>{p.label}</span>
                                <span className="chat-model-option-transport">{p.transport}</span>
                              </span>
                              <span className="chat-model-option-detail">{p.detail}</span>
                            </button>
                          ))}
                        </motion.div>
                      </>
                    )}
                  </AnimatePresence>
                </div>

                <button
                  type="submit"
                  className="chat-send"
                  disabled={!input.trim()}
                  title="Send"
                >
                  ↑
                </button>
              </div>
            </div>
          </form>
        </div>
      </section>

      {/* ── Activity drawer ─────────────────────────────────── */}
      <AnimatePresence>
        {detailsOpen && (
          <>
            <div className="chat-drawer-backdrop" onClick={() => setDetailsOpen(false)} />
            <motion.aside
              className="chat-drawer"
              initial={{ x: 320, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: 320, opacity: 0 }}
              transition={{ duration: 0.28, ease: EASE }}
            >
              <div className="chat-drawer-head">
                <span>Operational activity</span>
                <button className="chat-drawer-close" onClick={() => setDetailsOpen(false)}>
                  ✕
                </button>
              </div>

              {lastGoal ? (
                <>
                  <div className="chat-drawer-section">
                    <span className="chat-drawer-label">Checklist</span>
                    <div className="chat-drawer-steps">
                      <Step done={stageIdx >= 1} label="Understood request" />
                      <Step done={stageIdx >= 2} label="Evidence requirements derived" />
                      <Step done={stageIdx >= 2} label="Executing plan" />
                      <Step done={stageIdx >= 3} label="Verified result" />
                    </div>
                  </div>                    <div className="chat-drawer-section">
                    <span className="chat-drawer-label">Context</span>
                    <div className="chat-drawer-facts">
                      <Fact k="Goal" v={lastGoal.goal ?? '—'} mono />
                      <Fact k="Run id" v={lastGoal.goalId ?? '—'} mono />
                      <Fact
                        k="Routing"
                        v={`${providerById(lastGoal.providerIntent ?? 'auto').label}${lastGoal.providerIntent && lastGoal.providerIntent !== 'auto' ? ' (preference)' : ''}`}
                      />
                      <Fact
                        k="Answered by"
                        v={
                          lastGoal.provider
                            ? `${lastGoal.provider} · ${shortModel(lastGoal.model)}`
                            : lastGoal.strategy === 'heuristic'
                              ? 'heuristic (no model)'
                              : '—'
                        }
                      />
                      <Fact k="Strategy" v={lastGoal.strategy ?? '—'} />
                      <Fact
                        k="Result"
                        v={
                          lastGoal.result?.text
                            ? lastGoal.result.text.length > 120
                              ? `${lastGoal.result.text.slice(0, 120)}…`
                              : lastGoal.result.text
                            : '—'
                        }
                        mono
                      />
                      <Fact k="Phases" v={lastGoal.phases != null ? String(lastGoal.phases) : '—'} />
                      <Fact k="Evidence verified" v={lastGoal.evidenceVerified != null ? String(lastGoal.evidenceVerified) : '—'} />
                      <Fact k="Status" v={lastGoal.stage ?? 'sent'} />
                      {lastGoal.error && <Fact k="Error" v={lastGoal.error} mono />}
                    </div>
                  </div>                    {lastGoal.channels && lastGoal.channels.length > 0 && (
                    <div className="chat-drawer-section">
                      <span className="chat-drawer-label">Evidence channels</span>
                      <div className="chat-drawer-providers">
                        {lastGoal.channels.map((c) => (
                          <span
                            key={c.channel}
                            className={`chat-drawer-provider ${c.verified ? '' : 'chat-drawer-provider--no'}`}
                          >
                            {c.verified ? '✓' : '✕'} {c.channel}
                          </span>
                        ))}
                      </div>
                    </div>
                  )}

                  <div className="chat-drawer-section">
                    <span className="chat-drawer-label">Providers</span>
                    <div className="chat-drawer-providers">
                      {PROVIDER_OPTIONS.filter((p) => p.id !== 'auto').map((p) => (
                        <span key={p.id} className={`chat-drawer-provider ${p.id === lastGoal.providerIntent ? 'chat-drawer-provider--sel' : ''}`}>
                          {p.label} <span className="chat-drawer-provider-t">{p.transport}</span>
                        </span>
                      ))}
                    </div>
                    <div className="chat-drawer-note">
                      The runtime routes by capability — a selection here is a client-side preference for this goal.
                    </div>
                  </div>
                </>
              ) : (
                <div className="chat-drawer-empty">
                  Send a goal to see FRIDAY's operational trail — what the cognitive runtime actually did.
                </div>
              )}
            </motion.aside>
          </>
        )}
      </AnimatePresence>
    </div>
  );
}

/* ── Pieces ─────────────────────────────────────────────────────────── */

/** Whether a completed goal carried a renderable result — when true the card
 * renders the classified result view instead of the fallback summary. */
function hasRenderableResult(p: ParsedEvent): boolean {
  const r = p.result;
  return !!r && r.kind !== 'empty' && !!r.text && !!r.text.trim();
}

/** The metadata-only fallback shown when the runtime produced no renderable
 * result (absent/empty) — never an invented answer. */
function completedFallback(p: ParsedEvent): string {
  return `Done — ${p.phases ?? '–'} phases, ${p.evidenceVerified ?? 0} evidence verified.`;
}

/** Drawer checklist index for the last goal's stage. Failed caps at 3 so
 * "Verified result" stays unchecked. A cancelled/paused run tracks by phases
 * completed (verify is skipped on a stop): up to "Executing plan", never
 * "Verified result". */
function stageToIdx(g: ChatMessage): number {
  if (g.stage === 'failed') return 3;
  if (g.stage === 'cancelled' || g.stage === 'paused') {
    const p = g.phases ?? 0;
    return p >= 2 ? 2 : p >= 1 ? 1 : 0;
  }
  return ['sent', 'understood', 'started', 'done', 'failed'].indexOf(g.stage ?? 'sent');
}

/** Short display for a fabric model id: `openrouter/nvidia/nemotron:free` →
 * `nvidia/nemotron:free` (the leading segment is always the provider). */
function shortModel(model?: string): string {
  if (!model) return '—';
  const slash = model.indexOf('/');
  return slash > 0 ? model.slice(slash + 1) : model;
}

function Step({ done, label }: { done: boolean; label: string }) {
  return (
    <div className={`chat-step ${done ? 'chat-step--done' : ''}`}>
      <span className="chat-step-icon">{done ? '✓' : '○'}</span>
      <span className="chat-step-label">{label}</span>
    </div>
  );
}

function Fact({ k, v, mono }: { k: string; v: string; mono?: boolean }) {
  return (
    <div className="chat-fact">
      <span className="chat-fact-k">{k}</span>
      <span className={`chat-fact-v ${mono ? 'chat-fact-v--mono' : ''}`}>{v}</span>
    </div>
  );
}

function GoalCard({
  msg,
  onCopy,
  onRegenerate,
  onAsk,
  askOpen,
  onCloseAsk,
  onPickAsk,
  onCancel,
  onPause,
  onApprove,
}: {
  msg: ChatMessage;
  onCopy: () => void;
  onRegenerate: () => void;
  onAsk: () => void;
  askOpen: boolean;
  onCloseAsk: () => void;
  onPickAsk: (intent: string) => void;
  onCancel: () => void;
  onPause: () => void;
  onApprove: () => void;
}) {
  const stage = msg.stage ?? 'sent';
  const done = stage === 'done';
  const failed = stage === 'failed';
  const cancelled = stage === 'cancelled';
  const paused = stage === 'paused';
  // A stop is terminal — the card is no longer "working", but it is neither a
  // success nor a failure. `working` drives the spinner + control buttons.
  const stopped = cancelled || paused;
  const working = !done && !failed && !stopped;
  const pulse = working ? 'chat-card-pulse' : '';
  const view = useMemo(() => classifyResult(msg.result), [msg.result]);

  return (
    <motion.div
      className="chat-row chat-row--friday"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.25, ease: [0.22, 1, 0.36, 1] }}
    >
      <div
        className={`chat-card ${done ? 'chat-card--done' : ''} ${failed ? 'chat-card--failed' : ''} ${cancelled ? 'chat-card--cancelled' : ''} ${paused ? 'chat-card--paused' : ''}`}
      >
        <div className="chat-card-head">
          <span className={`chat-card-avatar ${pulse}`}>◆</span>
          <span className="chat-card-name">FRIDAY</span>
          <span
            className={`chat-card-status ${done ? 'chat-card-status--done' : failed ? 'chat-card-status--failed' : cancelled ? 'chat-card-status--cancelled' : paused ? 'chat-card-status--paused' : 'chat-card-status--working'}`}
          >
            {working ? (
              <span className="chat-card-spinner" />
            ) : cancelled ? (
              '✕'
            ) : paused ? (
              '❚❚'
            ) : done ? (
              '✓'
            ) : (
              '✕'
            )}
            {done ? 'Completed' : failed ? 'Failed' : cancelled ? 'Cancelled' : paused ? 'Paused' : 'Working…'}
          </span>
        </div>

        <div className="chat-card-body">
          <ResultBody view={view} fallback={msg.text} />
        </div>

        {done && (msg.channels?.length || msg.phases != null || msg.evidenceVerified != null) && (
          <div className="chat-card-meta">
            {msg.evidenceVerified != null && (
              <span className="chat-card-meta-chip">
                <span className="chat-card-meta-dot chat-card-meta-dot--green" />
                {msg.evidenceVerified} evidence verified
              </span>
            )}
            {msg.phases != null && (
              <span className="chat-card-meta-chip">{msg.phases} phases</span>
            )}
            {msg.goalId && (
              <span className="chat-card-meta-chip chat-card-meta-chip--run" title={msg.goalId}>
                run {msg.goalId.slice(0, 13)}…
              </span>
            )}
            {msg.provider ? (
              <span className="chat-card-meta-chip chat-card-meta-chip--route" title="The model that answered the investigation">
                🧠 {msg.provider} · {shortModel(msg.model)}
              </span>
            ) : msg.strategy === 'heuristic' ? (
              <span className="chat-card-meta-chip chat-card-meta-chip--heuristic" title="Model unreachable — deterministic fallback planned">
                🧠 heuristic
              </span>
            ) : (
              <span className="chat-card-meta-chip">
                🧠 {providerById(msg.providerIntent ?? 'auto').label}
              </span>
            )}
          </div>
        )}

        {/* S3: a running goal carries its run id once started — the human can
         * pause or cancel it; the runtime turns the command into the real
         * goal.cancelled / goal.paused transition this card resolves to. */}
        {working && msg.goalId && (
          <div className="chat-card-controls">
            <button
              className="chat-card-control chat-card-control--pause"
              onClick={onPause}
              title="Pause the running goal"
            >
              ❚❚ Pause
            </button>
            <button
              className="chat-card-control chat-card-control--cancel"
              onClick={onCancel}
              title="Cancel the running goal"
            >
              ✕ Cancel
            </button>
          </div>
        )}

        {/* S3: a completed goal whose episode was held for a human — approve
         * admits it into memory (task.approve → goal.approved). */}
        {done && msg.pendingApprovalId && !msg.approved && (
          <div className="chat-card-approve">
            <div className="chat-card-approve-text">
              This result touches sensitive content — it was held for your
              review and not stored in memory.
            </div>
            <button
              className="chat-card-approve-btn"
              onClick={onApprove}
              title="Admit the held episode into memory"
            >
              Approve &amp; remember
            </button>
          </div>
        )}
        {done && msg.approved && (
          <div className="chat-card-approve chat-card-approve--ok">
            <span className="chat-card-approve-check">✓</span>
            Approved — the held episode was admitted to memory.
          </div>
        )}

        {done && msg.channels && msg.channels.length > 0 && (
          <div className="chat-card-channels">
            {msg.channels.map((c) => (
              <span
                key={c.channel}
                className={`chat-card-chan ${c.verified ? 'chat-card-chan--ok' : 'chat-card-chan--no'}`}
                title={c.verified ? 'Evidence verified' : 'Evidence failed'}
              >
                {c.verified ? '✓' : '✕'} {c.channel}
              </span>
            ))}
          </div>
        )}

        {failed && msg.error && <div className="chat-card-error">{msg.error}</div>}

        {!working && (
          <div className="chat-card-actions">
            <button className="chat-card-action" onClick={onCopy} title="Copy">
              Copy
            </button>
            <button className="chat-card-action" onClick={onRegenerate} title="Regenerate">
              ↻ Regenerate
            </button>
            <button className="chat-card-action" onClick={onAsk} title="Ask another model">
              ✦ Ask another model
            </button>
          </div>
        )}

        {!working && msg.result && <ExecutionDetails result={msg.result} />}

        <AnimatePresence>
          {askOpen && (
            <>
              <div className="chat-card-backdrop" onClick={onCloseAsk} />
              <motion.div
                className="chat-card-ask"
                initial={{ opacity: 0, y: -6, scale: 0.97 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: -6, scale: 0.97 }}
                transition={{ duration: 0.15, ease: [0.22, 1, 0.36, 1] }}
              >
                <div className="chat-card-ask-title">Ask via</div>
                {PROVIDER_OPTIONS.map((p) => (
                  <button
                    key={p.id}
                    className="chat-card-ask-opt"
                    onClick={() => onPickAsk(p.id)}
                  >
                    <span>{p.label}</span>
                    <span className="chat-card-ask-t">{p.transport}</span>
                  </button>
                ))}
                <div className="chat-card-ask-note">The runtime routes to the best expert for this goal.</div>
              </motion.div>
            </>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
}

/* ── Result renderer ─────────────────────────────────────────────────── */

/** Renders a classified result view. `fallback` (the metadata summary) shows
 * only when the runtime produced no renderable result — never an invented
 * answer, and never a raw JSON dump. */
function ResultBody({ view, fallback }: { view: ResultView; fallback: string }) {
  switch (view.type) {
    case 'empty':
      return <span>{fallback}</span>;
    case 'error':
      return <span className="chat-result-error">{view.text}</span>;
    case 'text':
      return <span>{view.text}</span>;
    case 'fileList':
      return <FileListView view={view} />;
    case 'array':
      return <ArrayListView view={view} />;
    case 'structured':
      return <StructuredView view={view} />;
    case 'raw':
      return <RawResultView data={view.data} />;
  }
}

/** A filesystem listing: intro line, one 📄/📁 row per entry, total footer. */
function FileListView({ view }: { view: Extract<ResultView, { type: 'fileList' }> }) {
  const lines = fileListLines(view.entries);
  const total = entryCountLabel(view.entries.length);
  if (view.entries.length === 0) {
    return (
      <div className="chat-card-result">
        <div className="chat-card-result-intro">No files found.</div>
      </div>
    );
  }
  return (
    <div className="chat-card-result">
      <div className="chat-card-result-intro">
        I found {total} in {view.path && view.path !== '.' ? `\`${view.path}\`` : 'this project'}:
      </div>
      <ul className="chat-file-list">
        {lines.map((line, i) => (
          <li key={`${line.name}-${i}`} className="chat-file-item">
            <span className="chat-file-icon">{line.icon === 'folder' ? '📁' : '📄'}</span>
            <span className="chat-file-name">{line.name}</span>
          </li>
        ))}
      </ul>
      <div className="chat-card-result-total">{total} total.</div>
    </div>
  );
}

/** A plain array result: one readable line per item. */
function ArrayListView({ view }: { view: Extract<ResultView, { type: 'array' }> }) {
  const lines = arrayLines(view.items);
  return (
    <div className="chat-card-result">
      <ul className="chat-array-list">
        {lines.map((line, i) => (
          <li key={i} className="chat-array-item">
            {line}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** A generic structured object: human-readable key → value rows (nested values
 * summarized; the raw payload is reachable via "Execution details"). */
function StructuredView({ view }: { view: Extract<ResultView, { type: 'structured' }> }) {
  const rows = structureRows(view.data);
  return (
    <div className="chat-card-result">
      <div className="chat-struct-rows">
        {rows.map((r) => (
          <div key={r.key} className="chat-struct-row">
            <span className="chat-struct-key">{r.key}</span>
            <span className="chat-struct-val">{r.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** An unclassifiable payload: formatted JSON behind a collapsible toggle. */
function RawResultView({ data, label = 'Raw result' }: { data: unknown; label?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="chat-raw">
      <button
        type="button"
        className="chat-card-details-toggle"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        <span className={`chat-card-caret ${open ? 'chat-card-caret--open' : ''}`}>▸</span>
        {label}
      </button>
      {open && <pre className="chat-card-raw">{JSON.stringify(data, null, 2)}</pre>}
    </div>
  );
}

/** The card's collapsible raw-result debugger: the exact `goal.completed.result`
 * payload the runtime emitted, formatted, never truncated. */
function ExecutionDetails({ result }: { result: GoalResult }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="chat-card-details">
      <button
        type="button"
        className="chat-card-details-toggle"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
      >
        <span className={`chat-card-caret ${open ? 'chat-card-caret--open' : ''}`}>▸</span>
        Execution details
      </button>
      {open && <pre className="chat-card-raw">{JSON.stringify(result, null, 2)}</pre>}
    </div>
  );
}

function EmptyState({ onPick }: { onPick: (goal: string) => void }) {
  const suggestions = [
    'Check the git status of the workspace repository',
    'Search the web for the latest Rust async docs',
    'List the files in this project',
    'Explain the provider routing architecture',
  ];
  return (
    <div className="chat-empty">
      <motion.div
        className="chat-empty-mark"
        initial={{ scale: 0.8, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
      >
        ◆
      </motion.div>
      <div className="chat-empty-title">Ask FRIDAY anything</div>
      <div className="chat-empty-sub">
        Goals run through the cognitive runtime — investigate → plan → execute → verify.
      </div>
      <div className="chat-empty-sugs">
        {suggestions.map((s) => (
          <button key={s} className="chat-empty-sug" onClick={() => onPick(s)}>
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}
