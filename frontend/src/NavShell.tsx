import { useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import {
  HomeScreen,
  ChatScreen,
  ProjectsScreen,
  MemoryScreen,
  TerminalScreen,
  ProvidersScreen,
  SettingsScreen,
  ConsoleDashboardScreen,
  ConsoleExecutionGraphScreen,
  ConsoleTimelineScreen,
  ConsoleExplanationScreen,
  ConsoleMetricsScreen,
  ConsoleLedgerScreen,
} from './screens';
import { useConnection } from './hooks/useProtocol';
import { StatusPill } from './components';
import './NavShell.css';

type Screen =
  | 'chat'
  | 'daemon'
  | 'projects'
  | 'memory'
  | 'terminal'
  | 'providers'
  | 'settings'
  | 'console-dashboard'
  | 'console-execution-graph'
  | 'console-timeline'
  | 'console-explanation'
  | 'console-metrics'
  | 'console-ledger';

/** Primary experience — the dashboard opens on Chat. */
const MAIN_ITEMS: { id: Screen; label: string; icon: string }[] = [
  { id: 'chat', label: 'Chat', icon: '◈' },
];

/** The daemon/system surfaces, kept as modules behind their own nav group. */
const SYSTEM_ITEMS: { id: Screen; label: string; icon: string }[] = [
  { id: 'daemon', label: 'Daemon', icon: '⌂' },
  { id: 'memory', label: 'Memory', icon: '❖' },
  { id: 'projects', label: 'Projects', icon: '▤' },
  { id: 'terminal', label: 'Terminal', icon: '❯_' },
  { id: 'providers', label: 'Providers', icon: '◎' },
  { id: 'settings', label: 'Settings', icon: '⚙' },
];

const CONSOLE_ITEMS: { id: Screen; label: string; icon: string }[] = [
  { id: 'console-dashboard', label: 'Dashboard', icon: '◎' },
  { id: 'console-execution-graph', label: 'Execution Graph', icon: '⌬' },
  { id: 'console-timeline', label: 'Timeline', icon: '≋' },
  { id: 'console-explanation', label: 'Explanation', icon: '◈' },
  { id: 'console-metrics', label: 'Metrics', icon: '▥' },
  { id: 'console-ledger', label: 'Ledger', icon: '▤' },
];

export function NavShell() {
  const [screen, setScreen] = useState<Screen>('chat');
  const conn = useConnection();

  return (
    <div className="nav-shell">
      {/* ── Sidebar ────────────────────────────────────── */}
      <nav className="nav-sidebar">
        <div className="nav-brand">
          <span className="nav-brand-icon">◆</span>
          <span className="nav-brand-text">FRIDAY</span>
        </div>

        <div className="nav-items">
          <span className="nav-group-label">Main</span>
          {MAIN_ITEMS.map((item) => (
            <button
              key={item.id}
              className={`nav-item nav-item--primary ${screen === item.id ? 'nav-item--active' : ''}`}
              onClick={() => setScreen(item.id)}
            >
              <span className="nav-item-icon">{item.icon}</span>
              <span className="nav-item-label">{item.label}</span>
            </button>
          ))}

          <span className="nav-group-label">System</span>

          {SYSTEM_ITEMS.map((item) => (
            <button
              key={item.id}
              className={`nav-item ${screen === item.id ? 'nav-item--active' : ''}`}
              onClick={() => setScreen(item.id)}
            >
              <span className="nav-item-icon">{item.icon}</span>
              <span className="nav-item-label">{item.label}</span>
            </button>
          ))}

          <span className="nav-group-label">Console</span>

          {CONSOLE_ITEMS.map((item) => (
            <button
              key={item.id}
              className={`nav-item ${screen === item.id ? 'nav-item--active' : ''}`}
              onClick={() => setScreen(item.id)}
            >
              <span className="nav-item-icon">{item.icon}</span>
              <span className="nav-item-label">{item.label}</span>
            </button>
          ))}
        </div>

        <div className="nav-footer">
          <StatusPill
            level={conn.status === 'connected' ? 'green' : 'red'}
            text={conn.status === 'connected' ? 'Connected' : conn.status}
            size="sm"
          />
        </div>
      </nav>

      {/* ── Content ────────────────────────────────────── */}
      <main className="nav-content">
        <AnimatePresence mode="wait">
          <motion.div
            key={screen}
            className="nav-content-inner"
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
          >
            {screen === 'chat' && <ChatScreen />}
            {screen === 'daemon' && <HomeScreen />}
            {screen === 'projects' && <ProjectsScreen />}
            {screen === 'memory' && <MemoryScreen />}
            {screen === 'terminal' && <TerminalScreen />}
            {screen === 'providers' && <ProvidersScreen />}
            {screen === 'settings' && <SettingsScreen />}
            {screen === 'console-dashboard' && <ConsoleDashboardScreen />}
            {screen === 'console-execution-graph' && <ConsoleExecutionGraphScreen />}
            {screen === 'console-timeline' && <ConsoleTimelineScreen />}
            {screen === 'console-explanation' && <ConsoleExplanationScreen />}
            {screen === 'console-metrics' && <ConsoleMetricsScreen />}
            {screen === 'console-ledger' && <ConsoleLedgerScreen />}
          </motion.div>
        </AnimatePresence>
      </main>

      {/* ── Ambient glow ──────────────────────────────── */}
      <div className="nav-ambient-glow" />
    </div>
  );
}
