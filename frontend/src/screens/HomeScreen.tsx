import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import { Card, StatusPill, MetricBar, SectionLabel, Input, ResponseCard, ResponseLine } from '../components';
import { useFriday } from '../hooks/useFriday';
import { useConnection, useEvents } from '../hooks/useProtocol';
import { greeting } from '../tokens';
import './HomeScreen.css';

export function HomeScreen() {
  const client = useFriday();
  const conn = useConnection();
  const events = useEvents(10);
  const [time, setTime] = useState(formatTime());
  const [goal, setGoal] = useState('');

  useEffect(() => {
    const timer = setInterval(() => setTime(formatTime()), 1000);
    return () => clearInterval(timer);
  }, []);

  const t = conn.telemetry;
  const recentTasks = events
    .filter((e) => e.parsed.task)
    .slice(0, 5);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!goal.trim()) return;
    client.sendGoal(goal.trim());
    setGoal('');
  };

  return (
    <div className="home">
      {/* ── Ambient header ──────────────────────────────── */}
      <motion.div
        className="home-ambient"
        initial={{ opacity: 0, y: -20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.8, ease: [0.22, 1, 0.36, 1] }}
      >
        <div className="home-time">{time}</div>
        <div className="home-greeting">{greeting()}, Sree</div>
        <div className="home-subtitle">
          {conn.status === 'connected'
            ? 'FRIDAY is monitoring your system.'
            : conn.status === 'reconnecting'
              ? 'Reconnecting...'
              : 'FRIDAY is offline.'}
        </div>
      </motion.div>

      {/* ── Dashboard grid ─────────────────────────────── */}
      <div className="home-grid">
        {/* System status */}
        <Card className="home-card">
          <SectionLabel>System</SectionLabel>
          <div className="home-status-row">
            <StatusPill
              level={conn.status === 'connected' ? 'green' : 'red'}
              text={conn.status === 'connected' ? 'Online' : conn.status}
            />
            {conn.health && (
              <StatusPill
                level={conn.health.latency_ms < 200 ? 'green' : conn.health.latency_ms < 500 ? 'amber' : 'red'}
                text={`${conn.health.latency_ms}ms`}
                size="sm"
              />
            )}
          </div>
        </Card>

        {/* Resources */}
        {t && (
          <Card className="home-card">
            <SectionLabel>Resources</SectionLabel>
            <div className="home-metrics">
              <MetricBar
                label="CPU"
                used={`${t.cpu_percent.toFixed(0)}%`}
                fraction={t.cpu_percent / 100}
              />
              <MetricBar
                label="RAM"
                used={`${t.memory_used_mb} / ${t.memory_total_mb} MB`}
                fraction={t.memory_total_mb > 0 ? t.memory_used_mb / t.memory_total_mb : 0}
              />
              <MetricBar
                label="Disk"
                used={`${t.disk_used_mb} / ${t.disk_total_mb} MB`}
                fraction={t.disk_total_mb > 0 ? t.disk_used_mb / t.disk_total_mb : 0}
              />
            </div>
          </Card>
        )}

        {/* Pending tasks */}
        <Card className="home-card">
          <SectionLabel>Tasks</SectionLabel>
          {recentTasks.length === 0 ? (
            <div className="home-empty">No pending tasks.</div>
          ) : (
            <div className="home-task-list">
              {recentTasks.map((e) => {
                const task = e.parsed.task!;
                return (
                  <ResponseLine
                    key={e.id}
                    icon={task.status === 'done' ? '✓' : task.status === 'running' ? '●' : task.status === 'failed' ? '✕' : '○'}
                    text={task.title}
                    color={
                      task.status === 'done' ? 'var(--fr-green)' :
                      task.status === 'running' ? 'var(--fr-primary)' :
                      task.status === 'failed' ? 'var(--fr-red)' :
                      'var(--fr-text-muted)'
                    }
                  />
                );
              })}
            </div>
          )}
        </Card>

        {/* Recent activity */}
        <Card className="home-card">
          <SectionLabel>Recent</SectionLabel>
          {events.length === 0 ? (
            <div className="home-empty">No activity yet.</div>
          ) : (
            <div className="home-task-list">
              {events.slice(0, 5).map((e) => (
                <ResponseLine
                  key={e.id}
                  icon="›"
                  text={e.parsed.summary}
                  color="var(--fr-text-dim)"
                />
              ))}
            </div>
          )}
        </Card>
      </div>

      {/* ── Ask anything bar ────────────────────────────── */}
      <form className="home-input" onSubmit={handleSubmit}>
        <Input
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          placeholder="Ask anything..."
          icon="›"
        />
      </form>
    </div>
  );
}

function formatTime(): string {
  const now = new Date();
  const h = now.getHours();
  const m = now.getMinutes().toString().padStart(2, '0');
  const ampm = h >= 12 ? 'PM' : 'AM';
  const h12 = h % 12 || 12;
  return `${h12}:${m} ${ampm}`;
}
