 
import React from "react";
import { Button } from "./Button";
 
interface SessionRestoreDialogProps {
  isOpen: boolean;
  lastSession: { items: string[] };
  onRestore: () => void;
  onStartFresh: () => void;
  onReview: () => void;
}
 
export function SessionRestoreDialog({
  isOpen,
  lastSession,
  onRestore,
  onStartFresh,
  onReview,
}: SessionRestoreDialogProps) {
  if (!isOpen) return null;
 
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center px-4">
      <div className="absolute inset-0 bg-lagado-modal-overlay" />
 
      <div className="relative bg-lagado-surface border border-lagado-border rounded-md p-6 max-w-md w-full shadow-2xl">
        <h2 className="text-h2 text-lagado-text-bright font-bold mb-3">
          Restore Previous Session?
        </h2>
 
        <p className="text-body text-lagado-text mb-4">
          Last session approved:
        </p>
 
        <div className="bg-lagado-surface-2 border border-lagado-border rounded-sm p-4 mb-6">
          <ul className="space-y-1 text-body-sm text-lagado-text">
            {lastSession.items.map((item, idx) => (
              <li key={idx} className="flex items-center gap-2">
                <span className="text-lagado-green">●</span>
                <span className="font-mono">{item}</span>
              </li>
            ))}
          </ul>
        </div>
 
        <div className="flex gap-3 flex-wrap">
          <Button variant="primary" size="md" onClick={onRestore} className="flex-1">
            Restore
          </Button>
          <Button variant="secondary" size="md" onClick={onStartFresh} className="flex-1">
            Start Fresh
          </Button>
          <Button variant="secondary" size="md" onClick={onReview} className="flex-1">
            Review
          </Button>
        </div>
      </div>
    </div>
  );
}
