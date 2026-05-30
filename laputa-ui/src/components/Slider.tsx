// Range slider component
 
interface SliderProps {
  min: number;
  max: number;
  value: number;
  onChange: (value: number) => void;
  label?: string;
  step?: number;
}
 
export function Slider({
  min,
  max,
  value,
  onChange,
  label,
  step = 1,
}: SliderProps) {
  return (
    <div>
      {label && (
        <div className="flex justify-between mb-2">
          <span className="text-body-sm text-laputa-text">{label}</span>
          <span className="text-body-sm text-laputa-text-bright font-mono">
            {value}
          </span>
        </div>
      )}
      <input
        type="range"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        step={step}
        className="w-full"
      />
    </div>
  );
}
