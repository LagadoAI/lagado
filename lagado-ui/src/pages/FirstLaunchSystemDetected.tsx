import React, { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";
import { Card } from "../components/Card";
import { Spinner } from "../components/Spinner";

interface FirstLaunchSystemDetectedProps {
  onNext: () => void;
}

interface SystemInfo {
  cpu_model: string;
  physical_cores: number;
  logical_threads: number;
  ram_total_gb: number;
  gpu_name: string | null;
  vram_total_mb: number | null;
  vram_free_mb: number | null;
  storage_free_gb: number;
  storage_total_gb: number;
  os: string;
  tier: string;
}

function Spec({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-4 py-2 border-b border-lagado-border/40 last:border-0">
      <span className="text-body-sm text-lagado-text-dim">{label}</span>
      <span className="text-body-sm text-lagado-text-bright font-mono text-right">{value}</span>
    </div>
  );
}

const recommendation = (s: SystemInfo): string => {
  const gpu = s.gpu_name ?? "Your CPU";
  const vram = s.vram_total_mb ? `${(s.vram_total_mb / 1024).toFixed(0)} GB` : "";
  switch (s.tier) {
    case "full":
      return `${gpu} (${vram}) comfortably runs the local 8B agent at a large context. You'll choose your model next.`;
    case "balanced":
      return `${gpu} (${vram}) runs the local 8B agent with a compact context, or the 1.2B for maximum speed. You'll choose next.`;
    case "light":
      return `${gpu} (${vram}) is best paired with the 1.2B model for fast, light assistance. You'll choose next.`;
    default:
      return `No discrete GPU detected — Lagado will run on your CPU with the 1.2B model. You'll choose next.`;
  }
};

export default function FirstLaunchSystemDetected({ onNext }: FirstLaunchSystemDetectedProps) {
  const navigate = useNavigate();
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    invoke<SystemInfo>("get_system_info")
      .then(setInfo)
      .catch(() => setErr("Couldn't read system info."));
  }, []);

  const handleNext = () => {
    onNext();
    navigate("/setup/models");
  };

  if (err) {
    return (
      <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
        <p className="text-lagado-red text-body-sm">{err}</p>
      </div>
    );
  }
  if (!info) {
    return (
      <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
        <div className="text-center">
          <Spinner size="lg" label="Reading your hardware…" />
          <p className="mt-6 text-body-sm text-lagado-text-dim max-w-sm">
            Probing real specs to recommend the best configuration
          </p>
        </div>
      </div>
    );
  }

  const vram = info.vram_total_mb
    ? `${(info.vram_total_mb / 1024).toFixed(0)} GB (${info.vram_total_mb.toLocaleString()} MiB)`
    : "—";

  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4 py-10">
      <div className="max-w-2xl w-full">
        <h1 className="text-h1 text-lagado-text-bright font-bold mb-2">System Detected</h1>
        <p className="text-body text-lagado-text-dim mb-8">
          Here's what Lagado found on your machine — read live, nothing assumed.
        </p>

        <Card>
          <Spec label="CPU" value={info.cpu_model} />
          <Spec label="Cores / Threads" value={`${info.physical_cores} / ${info.logical_threads}`} />
          <Spec label="RAM" value={`${info.ram_total_gb.toFixed(1)} GB`} />
          <Spec label="GPU" value={info.gpu_name ?? "none (CPU only)"} />
          <Spec label="GPU memory" value={vram} />
          <Spec
            label="Storage"
            value={`${info.storage_free_gb.toFixed(0)} GB free of ${info.storage_total_gb.toFixed(0)} GB`}
          />
          <Spec label="Operating system" value={info.os} />
        </Card>

        <div className="bg-lagado-purple/10 border border-lagado-purple/60 rounded-md p-4 my-6 flex items-start gap-3">
          <span className="text-xl">💡</span>
          <div>
            <p className="text-body text-lagado-purple font-semibold mb-1">Recommendation</p>
            <p className="text-body-sm text-lagado-text leading-relaxed">{recommendation(info)}</p>
          </div>
        </div>

        <div className="flex gap-3">
          <Button variant="secondary" size="lg" className="flex-1" onClick={() => navigate(-1)}>
            Back
          </Button>
          <Button variant="primary" size="lg" onClick={handleNext} className="flex-1">
            Continue
          </Button>
        </div>
      </div>
    </div>
  );
}
