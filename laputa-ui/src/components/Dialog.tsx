// Modal dialog component
 
interface DialogProps {
  isOpen: boolean;
  title: string;
  children: React.ReactNode;
  onClose: () => void;
  actions?: React.ReactNode;
}
 
export function Dialog({ isOpen, title, children, onClose, actions }: DialogProps) {
  if (!isOpen) return null;
 
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Overlay */}
      <div
        className="absolute inset-0 bg-lagado-modal-overlay"
        onClick={onClose}
      />
 
      {/* Dialog */}
      <div className="relative bg-lagado-surface border border-lagado-border rounded-sm p-6 max-w-md w-full mx-4 shadow-lg">
        <h2 className="text-h2 text-lagado-text-bright font-bold mb-4">
          {title}
        </h2>
        <div className="text-body text-lagado-text mb-6">{children}</div>
        {actions && <div className="flex gap-3">{actions}</div>}
      </div>
    </div>
  );
}
 
