// Radio button component
 
interface RadioProps {
  name: string;
  value: string;
  checked: boolean;
  onChange: () => void;
  label: string;
  description?: string;
}
 
export function Radio({
  name,
  value,
  checked,
  onChange,
  label,
  description,
}: RadioProps) {
  return (
    <label className="flex items-start gap-3 cursor-pointer p-3 rounded-sm hover:bg-laputa-surface-2 transition-colors">
      <input
        type="radio"
        name={name}
        value={value}
        checked={checked}
        onChange={onChange}
        className="w-4 h-4 mt-1 accent-laputa-red"
      />
      <div>
        <div className="text-body text-laputa-text-bright font-semibold">
          {label}
        </div>
        {description && (
          <div className="text-caption text-laputa-text-dim mt-1">
            {description}
          </div>
        )}
      </div>
    </label>
  );
}
 
