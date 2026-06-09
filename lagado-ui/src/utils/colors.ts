// Color utilities
 
export const colors = {
  bg: "#0f1117",
  surface: "#1a1c23",
  "surface-2": "#21242e",
  border: "#30363d",
  "border-light": "#3d4450",
  text: "#c8ccd4",
  "text-bright": "#ffffff",
  "text-dim": "#6b7280",
  red: "#e94560",
  purple: "#6a5acd",
  green: "#22c55e",
  yellow: "#f59e0b",
};
 
export function hexToRgb(
  hex: string
): { r: number; g: number; b: number } | null {
  const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  return result
    ? {
        r: parseInt(result[1], 16),
        g: parseInt(result[2], 16),
        b: parseInt(result[3], 16),
      }
    : null;
}
