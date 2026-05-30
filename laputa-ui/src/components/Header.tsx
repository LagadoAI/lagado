import React from 'react';

interface HeaderProps {
  title: string;
}

export function Header({ title }: HeaderProps) {
  return (
    <div className="bg-laputa-surface border-b border-laputa-border px-6 py-4">
      <h1 className="text-h1 text-laputa-text-bright font-bold">{title}</h1>
    </div>
  );
}
