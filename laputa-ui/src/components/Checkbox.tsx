// Checkbox component
 
interface CheckboxProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  className?: string;
}
 
export function Checkbox({ label, checked, onChange, className }: CheckboxProps) {
  return (
    <label className={`flex items-center gap-3 cursor-pointer ${className || ""}`}>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="w-4 h-4 accent-laputa-red"
      />
      <span className="text-body text-laputa-text">{label}</span>
    </label>
  );
}
 
