 
import React, { useState } from "react";
import { Button } from "./Button";
import { Checkbox } from "./Checkbox";
 
interface ClipboardAccessDialogProps {
  isOpen: boolean;
  clipboardContent: string;
  onDeny: () => void;
  onAllow: () => void;
}
 
export function ClipboardAccessDialog({
  isOpen,
  clipboardContent,
  onDeny,
  onAllow,
}: ClipboardAccessDialogProps) {
  const [sanitize, setSanitize] = useState(true);
  const [dontAskAgain, setDontAskAgain] = useState(false);
 
  if (!isOpen) return null;
 
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center px-4">
      <div className="absolute inset-0 bg-laputa-modal-overlay" />
 
      <div className="relative bg-laputa-surface border border-laputa-border rounded-md p-6 max-w-md w-full shadow-2xl">
        <h2 className="text-h2 text-laputa-text-bright font-bold mb-4">
          Permission: Clipboard Access
        </h2>
 
        <p className="text-body text-laputa-text mb-4">
          Laputa wants to read your clipboard.
        </p>
 
        <div className="mb-6">
          <p className="text-caption text-laputa-text-dim mb-2">Your clipboard contains:</p>
          <div className="bg-laputa-surface-2 border border-laputa-border rounded-sm p-3">
            <p className="text-body-sm text-laputa-text font-mono break-all">
              {clipboardContent.length > 100
                ? clipboardContent.substring(0, 100) + "..."
                : clipboardContent}{" "}
              <span className="text-laputa-text-dim">({clipboardContent.length} chars)</span>
            </p>
          </div>
        </div>
 
        <div className="space-y-3 mb-6">
          <Checkbox
            label="Sanitize sensitive content"
            checked={sanitize}
            onChange={setSanitize}
          />
          <Checkbox
            label="Don't ask again for this session"
            checked={dontAskAgain}
            onChange={setDontAskAgain}
          />
        </div>
 
        <div className="flex gap-3">
          <Button variant="secondary" size="md" onClick={onDeny} className="flex-1">
            Deny
          </Button>
          <Button variant="primary" size="md" onClick={onAllow} className="flex-1">
            Allow
          </Button>
        </div>
      </div>
    </div>
  );
}
 
