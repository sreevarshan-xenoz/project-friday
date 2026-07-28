import type { StatusLevel } from '../tokens';
import { statusColor } from '../tokens';
import './StatusPill.css';

interface StatusPillProps {
  level: StatusLevel;
  text: string;
  size?: 'sm' | 'md';
}

export function StatusPill({ level, text, size = 'md' }: StatusPillProps) {
  return (
    <span className={`fr-status-pill fr-status-pill--${size}`}>
      <span className="fr-status-dot" style={{ backgroundColor: statusColor(level) }} />
      <span style={{ color: statusColor(level) }}>{text}</span>
    </span>
  );
}
