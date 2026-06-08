// Security-critical dialog for URL handler requests
 
import React from "react";
import { Button } from "./Button";
 
interface URLHandlerConfirmationDialogProps {
  isOpen: boolean;
  source: string;
  action: string;
  parameters: { [key: string]: string };
  onDeny: () => void;
  onAllowOnce: () => void;
  onAlwaysAllow: () => void;
}
 
export function URLHandlerConfirmationDialog({
  isOpen,
  source,
  action,
  parameters,
  onDeny,
  onAllowOnce,
  onAlwaysAllow,
}: URLHandlerConfirmationDialogProps) {
  if (!isOpen) return null;
 
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center px-4">
      <div className="absolute inset-0 bg-lagado-modal-overlay" />
 
      <div className="relative bg-lagado-surface border border-lagado-yellow rounded-md p-6 max-w-md w-full shadow-2xl">
        <div className="flex items-center gap-3 mb-4">
          <span className="text-lagado-yellow text-3xl">⚠</span>
          <h2 className="text-h2 text-lagado-text-bright font-bold">
            Security: External Request
          </h2>
        </div>
 
        <p className="text-body text-lagado-text mb-4">
          An external application is requesting Lagado to perform an action.
        </p>
 
        <div className="space-y-3 mb-6">
          <div>
            <p className="text-caption text-lagado-text-dim">Source:</p>
            <p className="text-body-sm text-lagado-text-bright font-mono break-all">
              {source}
            </p>
          </div>
          <div>
            <p className="text-caption text-lagado-text-dim">Action:</p>
            <p className="text-body-sm text-lagado-red font-mono font-semibold">
              {action}
            </p>
          </div>
          <div>
            <p className="text-caption text-lagado-text-dim">Parameters:</p>
            <div className="bg-lagado-surface-2 border border-lagado-border rounded-sm p-3 mt-1">
              {Object.entries(parameters).map(([key, value]) => (
                <div key={key} className="text-body-sm font-mono">
                  <span className="text-lagado-text-dim">{key}:</span>{" "}
                  <span className="text-lagado-text">{value}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
 
        <div className="bg-lagado-yellow bg-opacity-10 border border-lagado-yellow rounded-sm p-3 mb-6">
          <p className="text-body-sm text-lagado-yellow">
            ⚠ Verify the source is trusted before allowing this action.
          </p>
        </div>
 
        <div className="flex gap-3 flex-wrap">
          <Button variant="primary" size="md" onClick={onDeny} className="flex-1">
            Deny
          </Button>
          <Button variant="secondary" size="md" onClick={onAllowOnce} className="flex-1">
            Allow Once
          </Button>
          <Button variant="secondary" size="md" onClick={onAlwaysAllow} className="flex-1">
            Always Allow
          </Button>
        </div>
      </div>
    </div>
  );
}
 
