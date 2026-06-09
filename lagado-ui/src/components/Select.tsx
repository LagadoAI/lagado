// Dropdown select component
 
interface SelectOption {
  value: string;
  label: string;
}
 
interface SelectProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  className?: string;
}
 
export function Select({ value, onChange, options, className }: SelectProps) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={`
        w-full px-3 py-2 rounded-sm
        bg-lagado-surface-2 border border-lagado-border
        text-lagado-text focus:border-lagado-red focus:outline-none
        font-rajdhani text-body
        transition-colors duration-mid
        ${className || ""}
      `}
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
  );
}
 
