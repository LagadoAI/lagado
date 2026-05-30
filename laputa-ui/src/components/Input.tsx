import React from 'react';

interface InputProps {
  value?: string;
  onChange?: (e: React.ChangeEvent<HTMLInputElement>) => void;
  placeholder?: string;
  type?: string;
  disabled?: boolean;
  className?: string;
  onKeyPress?: (e: React.KeyboardEvent<HTMLInputElement>) => void;
}

export function Input({ value, onChange, placeholder, type = 'text', disabled, className, onKeyPress }: InputProps) {
  return (
    <input
      type={type}
      value={value}
      onChange={onChange}
      onKeyPress={onKeyPress}
      placeholder={placeholder}
      disabled={disabled}
      className={`w-full px-3 py-2 bg-laputa-surface-2 border border-laputa-border rounded-sm text-laputa-text placeholder-laputa-text-dim focus:border-laputa-red focus:outline-none font-rajdhani text-body ${className || ''}`}
    />
  );
}
