import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge } from "../components/Badge";

interface Props {
  onNext: () => void;
}

const LIQUID_MODELS = [
  {
    filename: "LFM2.5-350M-Q4_K_M.gguf",
    label: "LFM2.5 — 350M",
    role: "Intent classifier / fast router",
    description: "Ultra-fast. Used as the clean-context intent classifier. Not recommended as the primary brain.",
    size: "~220 MB",
    ram: "~1 GB",
    speed: "Instant",
    tier: "router",
    recommended: false,
  },
  {
    filename: "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
    label: "LFM2.5 — 1.2B",
    role: "Light assistant",
    description: "Low-resource devices. Fast responses with limited reasoning depth.",
    size: "~800 MB",
    ram: "~2 GB",
    speed: "Very Fast",
    tier: "light",
    recommended: false,
  },
  {
    filename: "LFM2.5-8B-A1B-Q4_K_M.gguf",
    label: "LFM2.5 — 8B MoE",
    role: "Primary reasoning brain",
    description: "8B parameters, only 1B active (MoE). The main Lagado brain — reasoning, planning, and agent execution.",
    size: "~5 GB",
    ram: "~6 GB",
    speed: "Fast",
    tier: "main",
    recommended: true,
  },
  {
    filename: "LFM2.5-VL-450M-F16.gguf",
    label: "LFM2.5 — VL 450M",
    role: "Vision-language",
    description: "Visual understanding via SigLIP2 projector. Used alongside the 8B for screen comprehension.",
    size: "~850 MB",
    ram: "~2 GB",
    speed: "Fast",
    tier: "vision",
    recommended: false,
  },
];

export default function FirstLaunchModelSelection({ onNext }: Props) {
  const [selected, setSelected] = useState("LFM2.5-8B-A1B-Q4_K_M.gguf");
  const [available, setAvailable] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    invoke<string[]>("list_models").then(setAvailable).catch(() => {});
  }, []);

  const visibleModels = LIQUID_MODELS.filter(
    (m) => available.length === 0 || available.includes(m.filename)
  );

  const handleContinue = async () => {
    setSaving(true);
    setError("");
    try {
      await invoke("set_active_model", { filename: selected });
      onNext();
    } catch (e) {
      setError("Could not save model selection. You can change it later in Settings.");
      onNext(); // proceed anyway
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
      <div className="max-w-4xl w-full">

        <div className="mb-8">
          <h1 className="text-h1 text-lagado-text-bright font-bold mb-2">
            Choose Your Brain
          </h1>
          <p className="text-body text-lagado-text-dim">
            Lagado runs on <span className="text-lagado-text font-semibold">Liquid AI</span> —
            stateless foundation models built for tool use, not conversation.
            You can change this later in Settings.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-8">
          {visibleModels.map((model) => {
            const isSelected = selected === model.filename;
            const isUnavailable = available.length > 0 && !available.includes(model.filename);
            return (
              <div
                key={model.filename}
                onClick={() => !isUnavailable && setSelected(model.filename)}
                className={`
                  p-5 border rounded-sm transition-all
                  ${isUnavailable ? "opacity-40 cursor-not-allowed" : "cursor-pointer"}
                  ${isSelected
                    ? "border-lagado-red bg-lagado-red bg-opacity-5"
                    : "border-lagado-border bg-lagado-surface hover:border-lagado-border-light"
                  }
                `}
              >
                <div className="flex items-start justify-between mb-2">
                  <div>
                    {model.recommended && (
                      <div className="mb-2">
                        <Badge variant="success">RECOMMENDED</Badge>
                      </div>
                    )}
                    <h3 className="text-h3 text-lagado-text-bright font-bold">
                      {model.label}
                    </h3>
                    <p className="text-caption text-lagado-red font-medium mt-0.5">
                      {model.role}
                    </p>
                  </div>
                  <input
                    type="radio"
                    name="model"
                    checked={isSelected}
                    disabled={isUnavailable}
                    onChange={() => setSelected(model.filename)}
                    className="w-4 h-4 accent-lagado-red mt-1 flex-shrink-0"
                  />
                </div>

                <p className="text-body-sm text-lagado-text-dim mb-4">
                  {model.description}
                </p>

                <div className="grid grid-cols-3 gap-2 text-caption">
                  <div>
                    <span className="text-lagado-text-dim block">Size</span>
                    <span className="font-mono text-lagado-text">{model.size}</span>
                  </div>
                  <div>
                    <span className="text-lagado-text-dim block">RAM</span>
                    <span className="font-mono text-lagado-text">{model.ram}</span>
                  </div>
                  <div>
                    <span className="text-lagado-text-dim block">Speed</span>
                    <span className="font-mono text-lagado-text">{model.speed}</span>
                  </div>
                </div>

                {isUnavailable && (
                  <p className="text-caption text-lagado-text-dim mt-3 italic">
                    Not found in models directory
                  </p>
                )}
              </div>
            );
          })}
        </div>

        {error && (
          <p className="text-body-sm text-lagado-red mb-4">{error}</p>
        )}

        <div className="flex gap-3">
          <button
            onClick={handleContinue}
            disabled={saving}
            className="flex-1 py-3 bg-lagado-red text-white text-body-sm font-semibold rounded-md hover:bg-opacity-90 disabled:opacity-50 transition-colors"
          >
            {saving ? "Saving..." : "Continue"}
          </button>
          <button
            onClick={onNext}
            className="px-6 py-3 border border-lagado-border text-lagado-text-dim text-body-sm rounded-md hover:border-lagado-red hover:text-lagado-text transition-colors"
          >
            Custom Model
          </button>
        </div>

        <p className="text-caption text-lagado-text-dim mt-4 text-center">
          Models live in <span className="font-mono">~/.laputa-secure/models/</span>
        </p>
      </div>
    </div>
  );
}
