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
        className="absolute inset-0 bg-laputa-modal-overlay"
        onClick={onClose}
      />
 
      {/* Dialog */}
      <div className="relative bg-laputa-surface border border-laputa-border rounded-sm p-6 max-w-md w-full mx-4 shadow-lg">
        <h2 className="text-h2 text-laputa-text-bright font-bold mb-4">
          {title}
        </h2>
        <div className="text-body text-laputa-text mb-6">{children}</div>
        {actions && <div className="flex gap-3">{actions}</div>}
      </div>
    </div>
  );
}
 
