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
    <div className="border border-laputa-border rounded-lg bg-laputa-surface p-3 space-y-3 max-w-3xl mx-auto">
      {/* Action + Details toggle */}
      <div className="flex items-start gap-2">
        <span className="text-caption text-laputa-text-dim font-mono flex-1 break-all leading-relaxed">
          {req.action}
        </span>
        <button
          onClick={() => setExpanded(!expanded)}
          className="text-caption text-laputa-text-dim hover:text-laputa-text flex-shrink-0 transition-colors"
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
          className="w-full px-3 py-1.5 bg-laputa-surface-2 border border-laputa-border rounded-md text-body-sm text-laputa-text placeholder-laputa-text-dim focus:border-laputa-red focus:outline-none"
        />
      )}

      {/* Approve / Deny */}
      <div className="flex gap-2">
        <button
          onClick={onApprove}
          disabled={!approveEnabled}
          className={`px-4 py-1.5 rounded-md text-body-sm font-semibold transition-colors ${
            approveEnabled
              ? "bg-laputa-green text-white hover:bg-opacity-90"
              : "bg-laputa-surface-2 border border-laputa-border text-laputa-text-dim cursor-not-allowed"
          }`}
        >
          Approve
        </button>
        <button
          onClick={onDeny}
          className="px-4 py-1.5 rounded-md text-body-sm font-semibold bg-laputa-surface-2 border border-laputa-border text-laputa-text hover:border-laputa-red hover:text-laputa-red transition-colors"
        >
          Deny
        </button>
      </div>

      {/* Expanded details */}
      {expanded && (
        <div className="space-y-2 pt-2 border-t border-laputa-border">
          <div>
            <span className="text-caption text-laputa-text-dim font-semibold">Why: </span>
            <span className="text-caption text-laputa-text">{req.reason}</span>
          </div>
          <div>
            <span className="text-caption text-laputa-text-dim font-semibold">Where: </span>
            <span className="text-caption text-laputa-text">
              {req.origin_agent} in {req.origin_surface}
            </span>
          </div>
          <button
            onClick={() => onSwitch(req.origin_surface)}
            className="text-caption text-laputa-purple hover:text-laputa-text transition-colors"
          >
            Switch to {req.origin_surface} ↗
          </button>
        </div>
      )}
    </div>
  );
}
