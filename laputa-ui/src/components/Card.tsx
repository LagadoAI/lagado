import React from 'react';

interface CardProps {
  children: React.ReactNode;
  className?: string;
}

export function Card({ children, className }: CardProps) {
  return (
    <div className={`bg-laputa-surface border border-laputa-border rounded-sm p-6 ${className || ''}`}>
      {children}
    </div>
  );
}

