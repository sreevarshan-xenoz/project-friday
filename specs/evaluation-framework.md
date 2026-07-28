# Evaluation Framework Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-core` (event bus, memory), all engines
**Crate:** `services/evaluation` — crate `friday-evaluation`

## 1. Purpose

Great cognitive systems are not just modular. They're **measurable**.

Every engine in FRIDAY's architecture should have metrics that tell you
whether it's actually getting smarter or simply getting more complicated.

This framework defines how we measure every engine, every mission, and
every long-term trend. Without it, we're building blind.

## 2. What we measure

### 2.1 Per-engine metrics

Every engine exposes metrics via `EngineMetrics`. This framework
aggregates them into actionable dashboards.

### 2.2 Per-mission metrics

Every mission gets a scorecard after completion:

```rust
pub struct MissionScorecard {
    pub mission_id: String,
    pub goal: String,
    pub success: bool,
    pub duration_ms: u64,
    pub steps_total: usize,
    pub steps_completed: usize,
    pub steps_failed: usize,
    pub retries_used: usize,
    pub recoveries_succeeded: usize,
    pub cost_tokens: u64,
    pub cost_usd: f64,
    pub user_interventions: usize,  // how many times user had to help
    pub reflection_lessons: usize,
}
```

### 2.3 Long-term trends

Weekly/monthly aggregates that answer: **is FRIDAY getting better?**

## 3. Evaluation Areas

### 3.1 Planning

| Metric | What it measures | How to compute |
|--------|-----------------|----------------|
| `plan.success_rate` | % of plans that complete successfully | successful_missions / total_missions |
| `plan.steps_avg` | Average steps per plan | total_steps / total_missions |
| `plan.replan_rate` | How often plans need revision | replans / total_missions |
| `plan.decomposition_quality` | Are goals broken into right-sized steps | avg_step_duration < threshold |

**Target:** 90%+ success rate, <20% replan rate.

### 3.2 Memory

| Metric | What it measures | How to compute |
|--------|-----------------|----------------|
| `memory.recall_precision` | Of recalled memories, how many are relevant | relevant_recall / total_recall |
| `memory.recall_recall` | Of relevant memories, how many were recalled | relevant_recall / total_relevant |
| `memory.recall_latency_ms` | How fast memory retrieval is | avg(query_time) |
| `memory.storage_size` | Total memory footprint | bytes used |
| `memory.forget_rate` | How often memories are pruned | prunes / total_memories |
| `memory.consolidation_rate` | Short-term → long-term promotion rate | promoted / eligible |

**Target:** Precision >80%, recall latency <50ms.

### 3.3 Tool Use

| Metric | What it measures | How to compute |
|--------|-----------------|----------------|
| `tool.success_rate` | % of tool calls that succeed | successful_calls / total_calls |
| `tool.recovery_rate` | % of failures that are recovered | recovered_failures / total_failures |
| `tool.avg_retries` | Average retries before success or give up | total_retries / total_calls |
| `tool.selection_accuracy` | Did the first tool choice work? | first_choice_success / total_calls |
| `tool.permission_prompt_rate` | How often user is asked for permission | permission_prompts / total_calls |
| `tool.cost_per_task` | Average tokens/cost per task | total_cost / total_tasks |

**Target:** 95%+ success rate, <1.5 avg retries.

### 3.4 Learning

| Metric | What it measures | How to compute |
|--------|-----------------|----------------|
| `learning.mistake_reduction` | Are repeated mistakes decreasing? | mistakes_this_week / mistakes_last_week |
| `learning.preference_accuracy` | Does FRIDAY know what the user likes? | correct_preference_predictions / total_predictions |
| `learning.pattern_detection_rate` | How many patterns discovered | patterns_detected / opportunities |
| `learning.knowledge_growth` | Is knowledge base growing? | new_knowledge_entries / week |
| `learning.procedural_learning` | Are error-fix lessons being applied? | applied_fixes / available_fixes |

**Target:** Mistake reduction >10% week-over-week.

### 3.5 Autonomy

| Metric | What it measures | How to compute |
|--------|-----------------|----------------|
| `autonomy.completion_without_help` | Goals achieved without user intervention | unassisted_completions / total_completions |
| `autonomy.intervention_rate` | How often user has to step in | interventions / total_actions |
| `autonomy.proactive_actions` | Actions taken without being asked | proactive_actions / total_actions |
| `autonomy.mission_duration` | How long missions take | avg(mission_duration) |
| `autonomy.multi_day_success` | Multi-day missions that complete | completed / started |

**Target:** 80%+ completion without help.

### 3.6 Human Factors

| Metric | What it measures | How to compute |
|--------|-----------------|----------------|
| `ux.explanation_quality` | Does FRIDAY explain why it did something? | explained_actions / total_actions |
| `ux.interruption_appropriateness` | Were interruptions necessary? | appropriate_interruptions / total_interruptions |
| `ux.response_latency` | Time from user input to first response | avg(response_time_ms) |
| `ux.user_satisfaction` | Explicit user feedback | avg(thumbs_up / total_feedback) |
| `ux.trust_score` | Does user accept FRIDAY's suggestions? | accepted_suggestions / total_suggestions |

**Target:** >70% user satisfaction, <20% unnecessary interruptions.

### 3.7 Performance

| Metric | What it measures | How to compute |
|--------|-----------------|----------------|
| `perf.cpu_usage` | Average CPU utilization | avg(cpu_pct) |
| `perf.memory_usage` | RAM consumption | avg(ram_mb) |
| `perf.response_latency_p50` | Median response time | p50(response_times) |
| `perf.response_latency_p99` | 99th percentile response time | p99(response_times) |
| `perf.cycle_rate` | Cognitive loop cycles per second | cycles / second |
| `perf.event_throughput` | Events processed per second | events / second |
| `perf.storage_growth` | How fast storage grows | bytes_added / day |

**Target:** <500ms p50 response, <2GB RAM, <10% CPU idle.

## 4. Dashboard

A real-time dashboard (web UI) showing:

```
┌─────────────────────────────────────────────────────────────┐
│                    FRIDAY Evaluation Dashboard               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Planning          Memory           Tool Use                │
│  ┌──────────┐     ┌──────────┐    ┌──────────┐            │
│  │ 92% ✓    │     │ 85% prec │    │ 96% succ │            │
│  │ success  │     │ 32ms lat │    │ 1.2 retr │            │
│  └──────────┘     └──────────┘    └──────────┘            │
│                                                             │
│  Learning          Autonomy         Human Factors           │
│  ┌──────────┐     ┌──────────┐    ┌──────────┐            │
│  │ -15%     │     │ 78%      │    │ 82%      │            │
│  │ mistakes │     │ unassist │    │ satisf.  │            │
│  └──────────┘     └──────────┘    └──────────┘            │
│                                                             │
│  Performance                                                │
│  ┌──────────────────────────────────────────────┐          │
│  │ CPU: 3%  RAM: 847MB  Latency: 320ms p50     │          │
│  │ Cycles: 10/s  Events: 245/s  Storage: +2MB/d │          │
│  └──────────────────────────────────────────────┘          │
│                                                             │
│  Weekly Trend:                                              │
│  ┌──────────────────────────────────────────────┐          │
│  │ Planning: ████████████████░░░░ 82% → 92% ↑   │          │
│  │ Memory:   █████████████████░░░ 78% → 85% ↑   │          │
│  │ Tools:    ██████████████████░░ 91% → 96% ↑   │          │
│  │ Learning: ████████████████░░░░ -8% → -15% ↑  │          │
│  └──────────────────────────────────────────────┘          │
└─────────────────────────────────────────────────────────────┘
```

## 5. Evaluation events

```rust
pub enum EvaluationEvent {
    /// Mission completed — compute scorecard
    MissionCompleted {
        mission_id: String,
        scorecard: MissionScorecard,
    },

    /// Weekly aggregate computed
    WeeklyTrend {
        week: String,
        metrics: HashMap<String, f64>,
        trends: HashMap<String, TrendDirection>,
    },

    /// Alert: metric dropped below threshold
    MetricAlert {
        metric: String,
        current: f64,
        threshold: f64,
        severity: AlertSeverity,
    },

    /// Benchmark run completed
    BenchmarkCompleted {
        benchmark_id: String,
        results: HashMap<String, f64>,
    },
}
```

## 6. Benchmarks

Standardized tests that can be run repeatedly:

| Benchmark | What it tests | Success criteria |
|-----------|--------------|-----------------|
| `build_and_test` | Planning + Tool Use + Agent Runtime | Build succeeds, tests pass |
| `error_recovery` | Agent Runtime + Memory + Knowledge | Error encountered → fix applied → success |
| `multi_step_mission` | Mission Engine + Planning | 5-step mission completes without intervention |
| `memory_retention` | Memory Engine | Recall precision >80% after 1000 memories |
| `preference_learning` | Learning Engine | Correctly predicts user preference after 10 examples |
| `provider_failover` | Provider Engine | Seamless failover with <2s downtime |
| `cold_start` | System startup | Full system ready in <5s |
| `long_running` | All engines | 24h continuous operation without degradation |

## 7. Implementation

### 7.1 Metrics collection

Every engine emits metrics to the event bus on a configurable interval:

```rust
// In every engine:
fn metrics(&self) -> EngineMetrics {
    EngineMetrics {
        engine_name: "planning".into(),
        cycle_count: self.cycles,
        success_rate: self.successes as f64 / self.total as f64,
        average_latency_ms: self.total_latency / self.total as f64,
        error_count: self.errors,
        last_active: self.last_active,
        custom: HashMap::new(),
    }
}
```

### 7.2 Metrics storage

Metrics are stored in a time-series format (SQLite with timestamp index)
for historical analysis and trend computation.

### 7.3 Dashboard

The existing Desktop UI (`frontend/`) gets an evaluation dashboard page.
Metrics are served via the existing WebSocket protocol.

## 8. Acceptance criteria

- [ ] `friday-evaluation` builds clean (clippy/fmt/tests)
- [ ] Every engine emits `EngineMetrics` on the event bus
- [ ] Mission scorecards are generated after every mission
- [ ] Weekly trends are computed automatically
- [ ] Metric alerts fire when thresholds are breached
- [ ] Benchmarks can be run as CLI commands
- [ ] Dashboard shows real-time metrics in the Desktop UI
- [ ] Historical data is persisted for trend analysis
- [ ] Test: run benchmark suite → all metrics within target ranges

## 9. What this unlocks

- **Visibility:** know exactly how FRIDAY is performing
- **Accountability:** every engine's contribution is measurable
- **Improvement tracking:** is FRIDAY getting better or worse?
- **Debuggability:** when something breaks, metrics show where
- **Research validation:** prove that new features actually help
- **User trust:** show users that FRIDAY learns and improves
