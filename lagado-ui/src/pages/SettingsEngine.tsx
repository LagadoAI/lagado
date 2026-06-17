import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Card } from "../components/Card";
import { Button } from "../components/Button";

interface EngineStatus {
  ok: boolean;
  error?: string;
  model?: {
    file: string;
    arch: string;
    context_length: number | null;
    block_count: number | null;
    embedding_length: number | null;
    expert_count: number;
    is_moe: boolean;
    file_mb: number;
  };
  hardware?: {
    gpu: string | null;
    vram_total_mb: number | null;
    vram_free_mb: number | null;
    cpu_cores: number;
  };
  plan?: {
    ctx: number;
    n_gpu_layers: number;
    cpu_moe: boolean;
    predicted_vram_mb: number | null;
    feasibility: string | null;
    rationale: string;
  };
}

const feasColor = (f: string | null | undefined) =>
  f === "Green"
    ? "text-green-400"
    : f === "Amber"
    ? "text-yellow-400"
    : f === "Red"
    ? "text-lagado-red"
    : "text-lagado-text-dim";

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex justify-between py-1.5 border-b border-lagado-border/40 last:border-0">
      <span className="text-body-sm text-lagado-text-dim">{label}</span>
      <span className="text-body-sm text-lagado-text-bright font-mono">{value}</span>
    </div>
  );
}

export default function SettingsEngine() {
  const [s, setS] = useState<EngineStatus | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = () => {
    setLoading(true);
    invoke<EngineStatus>("get_engine_status")
      .then(setS)
      .catch(() => setS({ ok: false, error: "failed to read engine status" }))
      .finally(() => setLoading(false));
  };
  useEffect(refresh, []);

  if (!s) return <p className="text-lagado-text-dim text-body-sm">Reading engine…</p>;
  if (!s.ok)
    return <p className="text-lagado-red text-body-sm">Engine error: {s.error}</p>;

  const m = s.model!;
  const h = s.hardware!;
  const p = s.plan!;

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <p className="text-caption text-lagado-text-dim max-w-lg">
          Every value below is read from the model file or probed from your hardware —
          nothing is assumed. The plan adapts to whatever model and machine you run.
        </p>
        <Button onClick={refresh} disabled={loading}>
          {loading ? "…" : "Refresh"}
        </Button>
      </div>

      <Card>
        <h2 className="text-h2 text-lagado-text-bright font-bold mb-4">Discovered Model</h2>
        <Row label="File" value={m.file} />
        <Row label="Architecture" value={m.arch} />
        <Row label="Context window" value={m.context_length?.toLocaleString() ?? "—"} />
        <Row label="Layers" value={m.block_count ?? "—"} />
        <Row label="Embedding dim" value={m.embedding_length ?? "—"} />
        <Row
          label="Mixture-of-Experts"
          value={m.is_moe ? `yes (${m.expert_count} experts)` : "no"}
        />
        <Row label="Weights" value={`${m.file_mb.toLocaleString()} MiB`} />
      </Card>

      <Card>
        <h2 className="text-h2 text-lagado-text-bright font-bold mb-4">Hardware</h2>
        <Row label="GPU" value={h.gpu ?? "none (CPU only)"} />
        <Row
          label="VRAM free / total"
          value={
            h.vram_total_mb != null
              ? `${h.vram_free_mb?.toLocaleString()} / ${h.vram_total_mb.toLocaleString()} MiB`
              : "—"
          }
        />
        <Row label="CPU cores" value={h.cpu_cores} />
      </Card>

      <Card>
        <h2 className="text-h2 text-lagado-text-bright font-bold mb-4">Derived Plan</h2>
        <Row label="Context" value={p.ctx.toLocaleString()} />
        <Row
          label="GPU layers"
          value={`${p.n_gpu_layers}${m.block_count ? ` / ${m.block_count}` : ""}`}
        />
        <Row label="CPU-MoE" value={p.cpu_moe ? "on" : "off"} />
        <Row
          label="Predicted VRAM"
          value={
            p.predicted_vram_mb != null
              ? `${Math.round(p.predicted_vram_mb).toLocaleString()} MiB`
              : "— (cold start, calibrates on first launch)"
          }
        />
        <Row
          label="Feasibility"
          value={<span className={feasColor(p.feasibility)}>{p.feasibility ?? "—"}</span>}
        />
        <p className="text-caption text-lagado-text-dim mt-3 leading-relaxed">{p.rationale}</p>
      </Card>
    </div>
  );
}
