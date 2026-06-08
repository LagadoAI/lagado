// Loading spinner
 
interface SpinnerProps {
  size?: "sm" | "md" | "lg";
  label?: string;
}
 
export function Spinner({ size = "md", label }: SpinnerProps) {
  const sizeClasses = {
    sm: "w-4 h-4",
    md: "w-8 h-8",
    lg: "w-12 h-12",
  };
 
  return (
    <div className="flex flex-col items-center justify-center gap-3">
      <div
        className={`
          border-2 border-lagado-border rounded-full
          border-t-lagado-red animate-spin
          ${sizeClasses[size]}
        `}
      />
      {label && <p className="text-body text-lagado-text-dim">{label}</p>}
    </div>
  );
}
