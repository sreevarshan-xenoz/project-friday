import type { ReactNode } from 'react';
import type { StatusLevel } from '../tokens';
import './ui.css';

export function Panel({ title, actions, children, className = '' }: { title?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`con-panel ${className}`}>
      {(title || actions) && (
        <header className="con-panel-head">
          <h4 className="con-panel-title">{title}</h4>
          {actions}
        </header>
      )}
      <div className="con-panel-body">{children}</div>
    </section>
  );
}

export function Stat({ label, value, hint, tone }: { label: string; value: ReactNode; hint?: string; tone?: StatusLevel }) {
  return (
    <div className="con-stat">
      <span className="con-stat-label">{label}</span>
      <span className={`con-stat-value${tone ? ` con-stat-value--${tone}` : ''}`}>{value}</span>
      {hint && <span className="con-stat-hint">{hint}</span>}
    </div>
  );
}

export function Bar({ value, tone = 'primary', label }: { value: number; tone?: 'primary' | 'green' | 'amber' | 'red' | 'secondary'; label?: string }) {
  const pct = Math.round(Math.min(1, Math.max(0, value)) * 100);
  return (
    <div className="con-bar">
      <div className="con-bar-track">
        <div className={`con-bar-fill con-bar-fill--${tone}`} style={{ width: `${pct}%` }} />
      </div>
      {label && <span className="con-bar-label">{label}</span>}
    </div>
  );
}

export function Chip({ children, tone = 'default' }: { children: ReactNode; tone?: 'default' | 'green' | 'amber' | 'red' | 'primary' | 'secondary' }) {
  return <span className={`con-chip con-chip--${tone}`}>{children}</span>;
}

export function KeyVal({ k, v }: { k: ReactNode; v: ReactNode }) {
  return (
    <div className="con-keyval">
      <span className="con-keyval-key">{k}</span>
      <span className="con-keyval-value">{v}</span>
    </div>
  );
}

export function ScoreRow({ name, score, max }: { name: ReactNode; score: number; max: number }) {
  const v = max > 0 ? score / max : 0;
  const tone = v >= 0.66 ? 'green' : v >= 0.4 ? 'amber' : 'red';
  return (
    <div className="con-scorerow">
      <span className="con-scorerow-name">{name}</span>
      <div className="con-scorerow-bar">
        <Bar value={v} tone={tone} />
      </div>
      <span className="con-scorerow-value">{score.toFixed(2)}</span>
    </div>
  );
}
