import React from 'react';

interface ButtonProps {
  variant?: 'primary' | 'secondary' | 'danger';
  size?: 'sm' | 'md' | 'lg';
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
}

export function Button({ variant = 'primary', size = 'md', children, onClick, disabled, className }: ButtonProps) {
  const baseStyle = 'font-rajdhani font-semibold rounded-sm transition-colors cursor-pointer';
  const variants = {
    primary: 'bg-laputa-red text-white hover:bg-opacity-90',
    secondary: 'bg-laputa-surface border border-laputa-border text-laputa-text hover:bg-laputa-surface-2',
    danger: 'bg-laputa-red text-white hover:bg-opacity-90',
  };
  const sizes = {
    sm: 'px-3 py-1 text-btn',
    md: 'px-4 py-2 text-body-sm',
    lg: 'px-6 py-3 text-body',
  };

  return (
    <button
      className={`${baseStyle} ${variants[variant]} ${sizes[size]} ${disabled ? 'opacity-50 cursor-not-allowed' : ''} ${className || ''}`}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}
