import type { ReactNode } from 'react';
import { motion } from 'framer-motion';
import './ChatBubble.css';

export type BubbleRole = 'user' | 'friday';

interface ChatBubbleProps {
  role: BubbleRole;
  children: ReactNode;
}

export function ChatBubble({ role, children }: ChatBubbleProps) {
  return (
    <motion.div
      className={`fr-bubble fr-bubble--${role}`}
      initial={{ opacity: 0, y: 12, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.3, ease: [0.22, 1, 0.36, 1] }}
    >
      {children}
    </motion.div>
  );
}
