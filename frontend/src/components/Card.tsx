import type { ReactNode } from 'react';
import { motion, type MotionProps } from 'framer-motion';
import './Card.css';

interface CardProps extends MotionProps {
  children: ReactNode;
  className?: string;
  glow?: boolean;
  padding?: 'sm' | 'md' | 'lg';
}

export function Card({ children, className = '', glow = false, padding = 'md', ...motion }: CardProps) {
  return (
    <motion.div
      className={`fr-card fr-card--${padding} ${glow ? 'fr-card--glow' : ''} ${className}`}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: [0.22, 1, 0.36, 1] }}
      {...motion}
    >
      {children}
    </motion.div>
  );
}
