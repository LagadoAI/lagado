 
import React, { useState } from "react";
import { Button } from "./Button";
 
interface ExitImmersiveDialogProps {
  isOpen: boolean;
  onCancel: () => void;
  onPauseAndLeave: () => void;
  onKeepRunningAndLeave: () => void;
}
 
export function ExitImmersiveDialog({
  isOpen,
  onCancel,
  onPauseAndLeave,
  onKeepRunningAndLeave,
}: ExitImmersiveDialogProps) {
  const [step, setStep] = useState<1 | 2>(1);
  const [shouldPause, setShouldPause] = useState(true);
 
  if (!isOpen) return null;
 
  const handleStep1Continue = () => {
    setStep(2);
  };
 
  const handleFinalConfirm = () => {
    if (shouldPause) {
      onPauseAndLeave();
    } else {
      onKeepRunningAndLeave();
    }
  };
 
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center px-4">
      <div className="absolute inset-0 bg-lagado-modal-overlay" />
 
      <div className="relative bg-lagado-surface border border-lagado-border rounded-md p-6 max-w-md w-full shadow-2xl">
        {step === 1 && (
          <>
            <h2 className="text-h2 text-lagado-text-bright font-bold mb-3">
              Pause agent before leaving?
            </h2>
            <p className="text-body text-lagado-text mb-6">
              The agent is currently running. Should it continue when you leave?
            </p>
            <div className="flex gap-3">
              <Button
                variant="secondary"
                size="md"
                onClick={() => {
                  setShouldPause(false);
                  handleStep1Continue();
                }}
                className="flex-1"
              >
                Keep Running
              </Button>
              <Button
                variant="primary"
                size="md"
                onClick={() => {
                  setShouldPause(true);
                  handleStep1Continue();
                }}
                className="flex-1"
              >
                Pause
              </Button>
            </div>
            <button
              onClick={onCancel}
              className="block w-full text-center text-caption text-lagado-text-dim mt-4 hover:underline"
            >
              Cancel
            </button>
          </>
        )}
 
        {step === 2 && (
          <>
            <h2 className="text-h2 text-lagado-text-bright font-bold mb-3">
              Ready to leave immersive mode?
            </h2>
            <p className="text-body text-lagado-text mb-6">
              You'll return to the chat page.
              {shouldPause && (
                <span className="block text-body-sm text-lagado-text-dim mt-2">
                  Agent will be paused.
                </span>
              )}
            </p>
            <div className="flex gap-3">
              <Button
                variant="secondary"
                size="md"
                onClick={() => setStep(1)}
                className="flex-1"
              >
                Back
              </Button>
              <Button
                variant="primary"
                size="md"
                onClick={handleFinalConfirm}
                className="flex-1"
              >
                Yes, Go to Chat
              </Button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
 
