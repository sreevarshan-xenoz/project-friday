import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Card, SectionLabel, Button, Terminal } from '../components';
import { useEvents } from '../hooks/useProtocol';
import './ProjectsScreen.css';

interface Project {
  id: string;
  name: string;
  description: string;
  icon: string;
  color: string;
}

const DEMO_PROJECTS: Project[] = [
  { id: 'friday', name: 'FRIDAY', description: 'The platform itself', icon: '◆', color: 'var(--fr-primary)' },
  { id: 'linux', name: 'Linux Server', description: 'Remote server management', icon: '▶', color: 'var(--fr-green)' },
  { id: 'minecraft', name: 'Minecraft', description: 'Game server & mods', icon: '◼', color: 'var(--fr-amber)' },
  { id: 'research', name: 'Research', description: 'Paper reading & notes', icon: '●', color: 'var(--fr-secondary)' },
  { id: 'college', name: 'College', description: 'Coursework & projects', icon: '◎', color: 'var(--fr-red)' },
];

type ProjectTab = 'overview' | 'memory' | 'tasks' | 'terminal' | 'logs';

export function ProjectsScreen() {
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<ProjectTab>('overview');
  const events = useEvents(30);

  const project = DEMO_PROJECTS.find((p) => p.id === selected);

  return (
    <div className="projects">
      <AnimatePresence mode="wait">
        {!project ? (
          <motion.div
            key="list"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="projects-list"
          >
            <h2 className="projects-title">Projects</h2>
            <div className="projects-grid">
              {DEMO_PROJECTS.map((p) => (
                <Card
                  key={p.id}
                  className="projects-item"
                  glow
                  onClick={() => setSelected(p.id)}
                  whileHover={{ scale: 1.02 }}
                  whileTap={{ scale: 0.98 }}
                  style={{ cursor: 'pointer' }}
                >
                  <div className="projects-item-icon" style={{ color: p.color }}>{p.icon}</div>
                  <div className="projects-item-name">{p.name}</div>
                  <div className="projects-item-desc">{p.description}</div>
                </Card>
              ))}
            </div>
          </motion.div>
        ) : (
          <motion.div
            key="detail"
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -20 }}
            className="projects-detail"
          >
            <div className="projects-detail-header">
              <Button variant="ghost" onClick={() => setSelected(null)}>
                ← Back
              </Button>
              <h2 className="projects-detail-title" style={{ color: project.color }}>
                <span>{project.icon}</span> {project.name}
              </h2>
            </div>

            <div className="projects-tabs">
              {(['overview', 'memory', 'tasks', 'terminal', 'logs'] as ProjectTab[]).map((t) => (
                <button
                  key={t}
                  className={`projects-tab ${tab === t ? 'projects-tab--active' : ''}`}
                  onClick={() => setTab(t)}
                >
                  {t.charAt(0).toUpperCase() + t.slice(1)}
                </button>
              ))}
            </div>

            <div className="projects-content">
              {tab === 'overview' && (
                <Card padding="lg">
                  <SectionLabel>About</SectionLabel>
                  <p className="projects-overview-text">{project.description}</p>
                  <div className="projects-overview-stats">
                    <div className="projects-stat">
                      <span className="projects-stat-value">12</span>
                      <span className="projects-stat-label">Tasks completed</span>
                    </div>
                    <div className="projects-stat">
                      <span className="projects-stat-value">3</span>
                      <span className="projects-stat-label">Active sessions</span>
                    </div>
                    <div className="projects-stat">
                      <span className="projects-stat-value">24</span>
                      <span className="projects-stat-label">Memories</span>
                    </div>
                  </div>
                </Card>
              )}

              {tab === 'memory' && (
                <Card padding="lg">
                  <SectionLabel>Memory Timeline</SectionLabel>
                  <div className="projects-empty">
                    Memory timeline will surface relevant context from FRIDAY's
                    experience engine.
                  </div>
                </Card>
              )}

              {tab === 'tasks' && (
                <Card padding="lg">
                  <SectionLabel>Recent Tasks</SectionLabel>
                  {events.length === 0 ? (
                    <div className="projects-empty">No tasks for this project yet.</div>
                  ) : (
                    <div className="projects-task-list">
                      {events.slice(0, 10).map((e) => (
                        <div key={e.id} className="projects-task-row">
                          <span className="projects-task-topic">{e.topic}</span>
                          <span className="projects-task-summary">{e.parsed.summary}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </Card>
              )}

              {tab === 'terminal' && (
                <Terminal
                  lines={[
                    { prefix: '$', text: `cd ${project.name.toLowerCase().replace(/\s+/g, '-')}` },
                    { prefix: '$', text: 'ls', color: 'var(--fr-text)' },
                    { text: 'README.md  src/  tests/', color: 'var(--fr-text-dim)' },
                  ]}
                />
              )}

              {tab === 'logs' && (
                <Card padding="lg">
                  <SectionLabel>Event Log</SectionLabel>
                  <div className="projects-empty">
                    Event logs for this project will appear here.
                  </div>
                </Card>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
