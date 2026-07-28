import { forwardRef, type InputHTMLAttributes } from 'react';
import './Input.css';

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  icon?: string;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ icon, className = '', ...props }, ref) => {
    return (
      <div className={`fr-input-wrap ${className}`}>
        {icon && <span className="fr-input-icon">{icon}</span>}
        <input
          ref={ref}
          className={`fr-input ${icon ? 'fr-input--has-icon' : ''}`}
          {...props}
        />
      </div>
    );
  }
);

Input.displayName = 'Input';
