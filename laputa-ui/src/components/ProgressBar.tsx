interface ProgressBarProps {
  value: number;
  max: number;
  label?: string;
  showPercent?: boolean;
  className?: string;
}

export function ProgressBar({ value, max, label, showPercent, className }: ProgressBarProps) {
  const percent = Math.min(100, (value / max) * 100);

  return (
    <div className={className || ''}>
      {label && <p className="text-body-sm text-laputa-text-dim mb-1">{label}</p>}
      <div className="w-full h-2 bg-laputa-surface-2 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full transition-all ${percent > 80 ? 'bg-laputa-red' : 'bg-laputa-green'}`}
          style={{ width: `${percent}%` }}
        />
      </div>
      {showPercent && <p className="text-caption text-laputa-text-dim mt-1">{Math.round(percent)}%</p>}
    </div>
  );
}
