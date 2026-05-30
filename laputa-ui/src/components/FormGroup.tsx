// Form field wrapper with label
 
interface FormGroupProps {
  label: string;
  children: React.ReactNode;
  required?: boolean;
  error?: string;
}
 
export function FormGroup({ label, children, required, error }: FormGroupProps) {
  return (
    <div className="mb-6">
      <label className="block text-body-sm text-laputa-text-bright font-semibold mb-2">
        {label}
        {required && <span className="text-laputa-red ml-1">*</span>}
      </label>
      {children}
      {error && (
        <p className="text-caption text-laputa-red mt-2">{error}</p>
      )}
    </div>
  );
}
 
