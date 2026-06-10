import React from "react";

export function HyperLoader({ size = 30 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 40 40" fill="none" role="img" aria-label="Thinking">
      <defs>
        <linearGradient id="hc-grad" gradientUnits="userSpaceOnUse" x1="4" y1="4" x2="36" y2="36">
          <stop offset="0" stopColor="var(--primary, #3b82f6)" />
          <stop offset="1" stopColor="var(--secondary, #8b5cf6)" />
        </linearGradient>
      </defs>
      <g className="hc-g hc-torus">
        <ellipse cx="20" cy="20" rx="15" ry="5.4" stroke="url(#hc-grad)" strokeWidth="1.6" opacity="0.65" />
      </g>
      <g className="hc-g hc-spin">
        <rect x="9" y="9" width="22" height="22" rx="2" stroke="url(#hc-grad)" strokeWidth="2" />
      </g>
      <g className="hc-g hc-invert">
        <rect x="9" y="9" width="22" height="22" rx="2" stroke="url(#hc-grad)" strokeWidth="2" />
      </g>
    </svg>
  );
}
