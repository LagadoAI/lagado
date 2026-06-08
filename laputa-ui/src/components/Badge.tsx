// Badge/tag component
 
interface BadgeProps {
  children: React.ReactNode;
  variant?: "default" | "success" | "error" | "warning";
  size?: "sm" | "md";
}
 
export function Badge({
  children,
  variant = "default",
  size = "sm",
}: BadgeProps) {
  const variantClasses = {
    default: "bg-lagado-surface-2 text-lagado-text border border-lagado-border",
    success: "bg-lagado-green bg-opacity-20 text-lagado-green",
    error: "bg-lagado-red bg-opacity-20 text-lagado-red",
    warning: "bg-lagado-yellow bg-opacity-20 text-lagado-yellow",
  };
 
  const sizeClasses = {
    sm: "px-2 py-1 text-caption",
    md: "px-3 py-1.5 text-body-sm",
  };
 
  return (
    <span
      className={`
        inline-block rounded-full font-semibold
        ${variantClasses[variant]}
        ${sizeClasses[size]}
      `}
    >
      {children}
    </span>
  );
}
