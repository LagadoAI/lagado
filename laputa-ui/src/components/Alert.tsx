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
    info: "bg-laputa-purple bg-opacity-10 border-laputa-purple text-laputa-purple",
    success:
      "bg-laputa-green bg-opacity-10 border-laputa-green text-laputa-green",
    warning:
      "bg-laputa-yellow bg-opacity-10 border-laputa-yellow text-laputa-yellow",
    error: "bg-laputa-red bg-opacity-10 border-laputa-red text-laputa-red",
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
