// Alert/notification component
 
type AlertType = "info" | "success" | "warning" | "error";
 
interface AlertProps {
  type: AlertType;
  title: string;
  message?: string;
  closeable?: boolean;
  onClose?: () => void;
}
 
export function Alert({
  type,
  title,
  message,
  closeable,
  onClose,
}: AlertProps) {
  const typeStyles = {
    info: "bg-lagado-purple bg-opacity-10 border-lagado-purple text-lagado-purple",
    success:
      "bg-lagado-green bg-opacity-10 border-lagado-green text-lagado-green",
    warning:
      "bg-lagado-yellow bg-opacity-10 border-lagado-yellow text-lagado-yellow",
    error: "bg-lagado-red bg-opacity-10 border-lagado-red text-lagado-red",
  };
 
  const typeIcons = {
    info: "ℹ",
    success: "✓",
    warning: "⚠",
    error: "✕",
  };
 
  return (
    <div className={`border rounded-sm p-4 ${typeStyles[type]}`}>
      <div className="flex items-start justify-between">
        <div className="flex items-start gap-3">
          <span className="text-lg">{typeIcons[type]}</span>
          <div>
            <div className="font-semibold text-body">{title}</div>
            {message && (
              <div className="text-body-sm opacity-80 mt-1">{message}</div>
            )}
          </div>
        </div>
        {closeable && (
          <button
            onClick={onClose}
            className="text-lg opacity-60 hover:opacity-100 transition-opacity"
          >
            ✕
          </button>
        )}
      </div>
    </div>
  );
}
