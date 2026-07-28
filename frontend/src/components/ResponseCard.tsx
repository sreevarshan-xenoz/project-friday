import type { ReactNode } from 'react';
import { motion } from 'framer-motion';
import './ResponseCard.css';

interface ResponseCardProps {
  icon?: string;
  title?: string;
  children: ReactNode;
  actions?: ReactNode;
}

/** A structured FRIDAY response card — replaces walls of text. */
export function ResponseCard({ icon, title, children, actions }: ResponseCardProps) {
  return (
    <motion.div
      className="fr-response"
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }}
    >
      {(icon || title) && (
        <div className="fr-response-header">
          {icon && <span className="fr-response-icon">{icon}</span>}
          {title && <span className="fr-response-title">{title}</span>}
        </div>
      )}
      <div className="fr-response-body">{children}</div>
      {actions && <div className="fr-response-actions">{actions}</div>}
    </motion.div>
  );
}

/** A single status line inside a ResponseCard. */
export function ResponseLine({ icon, text, color }: { icon: string; text: string; color?: string }) {
  return (
    <div className="fr-response-line">
      <span className="fr-response-line-icon" style={{ color: color ?? 'var(--fr-green)' }}>
        {icon}
      </span>
      <span>{text}</span>
    </div>
  );
}
