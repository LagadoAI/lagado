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
      className={`w-full px-3 py-2 bg-lagado-surface-2 border border-lagado-border rounded-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-red focus:outline-none font-rajdhani text-body ${className || ''}`}
    />
  );
}
