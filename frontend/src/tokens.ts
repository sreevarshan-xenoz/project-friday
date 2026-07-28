/** Design tokens as TypeScript constants for programmatic use. */

export const colors = {
  bg: '#070B14',
  surface: '#0D1321',
  surface1: '#141C2E',
  surface2: '#1A2540',
  surface3: '#213054',
  border: 'rgba(255, 255, 255, 0.06)',
  borderGlow: 'rgba(91, 200, 255, 0.12)',

  text: '#E6EDF6',
  textDim: '#8A94A6',
  textMuted: '#5A6478',

  primary: '#5BC8FF',
  primaryDim: 'rgba(91, 200, 255, 0.15)',
  secondary: '#9D7BFF',
  secondaryDim: 'rgba(157, 123, 255, 0.15)',

  green: '#3DDC84',
  greenDim: 'rgba(61, 220, 132, 0.15)',
  amber: '#FFB020',
  amberDim: 'rgba(255, 176, 32, 0.15)',
  red: '#FF5C5C',
  redDim: 'rgba(255, 92, 92, 0.15)',
} as const;

export const spacing = {
  1: 4, 2: 8, 3: 12, 4: 16, 5: 20, 6: 24, 8: 32, 10: 40, 12: 48, 16: 64,
} as const;

export const radius = {
  sm: 8, md: 12, lg: 16, xl: 20, pill: 9999,
} as const;

export type StatusLevel = 'green' | 'amber' | 'red' | 'standby' | 'unknown';
export type TaskStatus = 'done' | 'running' | 'waiting' | 'failed';

export function statusColor(level: StatusLevel): string {
  switch (level) {
    case 'green': return colors.green;
    case 'amber': return colors.amber;
    case 'red': return colors.red;
    case 'standby': return colors.textMuted;
    case 'unknown': return colors.textMuted;
  }
}

export function taskColor(status: TaskStatus): string {
  switch (status) {
    case 'done': return colors.green;
    case 'running': return colors.primary;
    case 'waiting': return colors.textMuted;
    case 'failed': return colors.red;
  }
}

export function taskGlyph(status: TaskStatus): string {
  switch (status) {
    case 'done': return '✓';
    case 'running': return '●';
    case 'waiting': return '○';
    case 'failed': return '✕';
  }
}

export function greeting(): string {
  const h = new Date().getHours();
  if (h < 6) return 'Good night';
  if (h < 12) return 'Good morning';
  if (h < 17) return 'Good afternoon';
  return 'Good evening';
}
