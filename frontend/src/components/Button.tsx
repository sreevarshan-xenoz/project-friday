import type { ButtonHTMLAttributes, ReactNode } from 'react';
import './Button.css';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
type ButtonSize = 'sm' | 'md' | 'lg';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: ReactNode;
  children: ReactNode;
}

export function Button({
  variant = 'primary',
  size = 'md',
  icon,
  children,
  className = '',
  ...props
}: ButtonProps) {
  return (
    <button
      className={`fr-btn fr-btn--${variant} fr-btn--${size} ${className}`}
      {...props}
    >
      {icon && <span className="fr-btn-icon">{icon}</span>}
      {children}
    </button>
  );
}

export function IconButton({
  variant = 'ghost',
  size = 'sm',
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant; size?: ButtonSize }) {
  return (
    <button
      className={`fr-btn fr-btn--icon fr-btn--${variant} fr-btn--${size} ${className}`}
      {...props}
    />
  );
}
