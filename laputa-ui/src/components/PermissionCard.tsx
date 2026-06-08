import { useState } from "react";
import type { PermissionRequest } from "@/hooks/useAgentSocket";

interface PermissionCardProps {
  req: PermissionRequest;
  onApprove: () => void;
  onDeny: () => void;
  onSwitch: (surface: string) => void;
}

export function PermissionCard({ req, onApprove, onDeny, onSwitch }: PermissionCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [typedConfirm, setTypedConfirm] = useState("");

  const isTyped = req.type === "typed";
  const approveEnabled = !isTyped || typedConfirm.trim().length > 0;

  return (
    <div className="border border-lagado-border rounded-lg bg-lagado-surface p-3 space-y-3 max-w-3xl mx-auto">
      {/* Action + Details toggle */}
      <div className="flex items-start gap-2">
        <span className="text-caption text-lagado-text-dim font-mono flex-1 break-all leading-relaxed">
          {req.action}
        </span>
        <button
          onClick={() => setExpanded(!expanded)}
          className="text-caption text-lagado-text-dim hover:text-lagado-text flex-shrink-0 transition-colors"
        >
          Details {expanded ? "▴" : "▾"}
        </button>
      </div>

      {/* Typed confirmation input */}
      {isTyped && (
        <input
          type="text"
          value={typedConfirm}
          onChange={(e) => setTypedConfirm(e.target.value)}
          placeholder="Type to confirm..."
          className="w-full px-3 py-1.5 bg-lagado-surface-2 border border-lagado-border rounded-md text-body-sm text-lagado-text placeholder-lagado-text-dim focus:border-lagado-red focus:outline-none"
        />
      )}

      {/* Approve / Deny */}
      <div className="flex gap-2">
        <button
          onClick={onApprove}
          disabled={!approveEnabled}
          className={`px-4 py-1.5 rounded-md text-body-sm font-semibold transition-colors ${
            approveEnabled
              ? "bg-lagado-green text-white hover:bg-opacity-90"
              : "bg-lagado-surface-2 border border-lagado-border text-lagado-text-dim cursor-not-allowed"
          }`}
        >
          Approve
        </button>
        <button
          onClick={onDeny}
          className="px-4 py-1.5 rounded-md text-body-sm font-semibold bg-lagado-surface-2 border border-lagado-border text-lagado-text hover:border-lagado-red hover:text-lagado-red transition-colors"
        >
          Deny
        </button>
      </div>

      {/* Expanded details */}
      {expanded && (
        <div className="space-y-2 pt-2 border-t border-lagado-border">
          <div>
            <span className="text-caption text-lagado-text-dim font-semibold">Why: </span>
            <span className="text-caption text-lagado-text">{req.reason}</span>
          </div>
          <div>
            <span className="text-caption text-lagado-text-dim font-semibold">Where: </span>
            <span className="text-caption text-lagado-text">
              {req.origin_agent} in {req.origin_surface}
            </span>
          </div>
          <button
            onClick={() => onSwitch(req.origin_surface)}
            className="text-caption text-lagado-purple hover:text-lagado-text transition-colors"
          >
            Switch to {req.origin_surface} ↗
          </button>
        </div>
      )}
    </div>
  );
}
