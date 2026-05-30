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
    default: "bg-laputa-surface-2 text-laputa-text border border-laputa-border",
    success: "bg-laputa-green bg-opacity-20 text-laputa-green",
    error: "bg-laputa-red bg-opacity-20 text-laputa-red",
    warning: "bg-laputa-yellow bg-opacity-20 text-laputa-yellow",
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
