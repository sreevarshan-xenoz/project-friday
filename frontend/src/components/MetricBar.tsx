import './MetricBar.css';

interface MetricBarProps {
  label: string;
  used: string;
  fraction: number;
  color?: string;
}

export function MetricBar({ label, used, fraction, color }: MetricBarProps) {
  const pct = Math.min(Math.max(fraction, 0), 1) * 100;

  // Auto-color based on threshold if no explicit color
  let barColor = color ?? 'var(--fr-primary)';
  if (!color) {
    if (pct > 90) barColor = 'var(--fr-red)';
    else if (pct > 70) barColor = 'var(--fr-amber)';
  }

  return (
    <div className="fr-metric">
      <div className="fr-metric-header">
        <span className="fr-metric-label">{label}</span>
        <span className="fr-metric-value">{used}</span>
      </div>
      <div className="fr-metric-track">
        <div
          className="fr-metric-fill"
          style={{ width: `${pct}%`, backgroundColor: barColor }}
        />
      </div>
    </div>
  );
}
