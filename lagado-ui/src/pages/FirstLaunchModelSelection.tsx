import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";

interface Props {
  onNext: () => void;
}

interface ModelDetail {
  file: string;
  arch?: string;
  context_length?: number | null;
  block_count?: number | null;
  expert_count?: number;
  is_moe?: boolean;
  size_mb?: number;
  fit?: string;
  error?: string;
}

const roleOf = (f: string, m: ModelDetail): string => {
  if (m.is_moe || /8b/i.test(f)) return "Primary agent brain";
  if (/colbert/i.test(f)) return "Retrieval embeddings";
  if (/(^|[-_])vl|vision|mmproj/i.test(f)) return "Vision encoder";
  if (/1\.2b|instruct/i.test(f)) return "Fast assistant / classifier";
  return "Model";
};

// Infra models (embeddings, vision) run automatically — they aren't the "brain" you pick.
const isSelectable = (f: string) => !/colbert|mmproj|(^|[-_])vl-|vision/i.test(f);

const fitInfo = (fit?: string): { label: string; cls: string } => {
  switch (fit) {
    case "fits":
      return { label: "fits your GPU", cls: "text-green-400" };
    case "tight":
      return { label: "tight on VRAM", cls: "text-yellow-400" };
    case "partial/cpu":
      return { label: "partial / CPU", cls: "text-lagado-red" };
    default:
      return { label: "CPU", cls: "text-lagado-text-dim" };
  }
};

const fmtSize = (mb?: number) =>
  mb == null ? "—" : mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
const fmtCtx = (c?: number | null) =>
  c == null ? "—" : c >= 1000 ? `${Math.round(c / 1000)}k` : `${c}`;

export default function FirstLaunchModelSelection({ onNext }: Props) {
  const [models, setModels] = useState<ModelDetail[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [recommended, setRecommended] = useState<string>("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    invoke<{ models: ModelDetail[] }>("get_models_detailed")
      .then((r) => {
        setModels(r.models);
        const sel = r.models.filter((m) => !m.error && isSelectable(m.file));
        const rec =
          sel.find((m) => m.fit === "fits") ?? sel.find((m) => m.fit === "tight") ?? sel[0];
        if (rec) {
          setRecommended(rec.file);
          setSelected(rec.file);
        }
      })
      .catch(() => setError("Couldn't read models."));
  }, []);

  const handleContinue = async () => {
    setSaving(true);
    try {
      await invoke("set_active_model", { filename: selected });
    } catch {
      /* proceed anyway; changeable in Settings */
    }
    onNext();
    setSaving(false);
  };

  const selectable = models.filter((m) => !m.error && isSelectable(m.file));
  const infra = models.filter((m) => !m.error && !isSelectable(m.file));

  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4 py-10">
      <div className="max-w-4xl w-full">
        <div className="mb-8">
          <h1 className="text-h1 text-lagado-text-bright font-bold mb-2">Choose Your Brain</h1>
          <p className="text-body text-lagado-text-dim">
            Lagado runs local, stateless models built for tool use. Sizes and context below
            are read from your actual model files. You can change this anytime in Settings.
          </p>
        </div>

        {selectable.length === 0 && !error && (
          <p className="text-body-sm text-lagado-text-dim mb-6">
            No models found in <span className="font-mono">~/.laputa-secure/models/</span>.
          </p>
        )}

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
          {selectable.map((m) => {
            const isSelected = selected === m.file;
            const fit = fitInfo(m.fit);
            return (
              <div
                key={m.file}
                onClick={() => setSelected(m.file)}
                className={`p-5 border rounded-md cursor-pointer transition-all ${
                  isSelected
                    ? "border-lagado-red bg-lagado-red/5"
                    : "border-lagado-border bg-lagado-surface hover:border-lagado-border-light"
                }`}
              >
                <div className="flex items-start justify-between mb-2">
                  <div>
                    {m.file === recommended && (
                      <div className="mb-2">
                        <Badge variant="success">RECOMMENDED</Badge>
                      </div>
                    )}
                    <h3 className="text-h3 text-lagado-text-bright font-bold break-all">
                      {m.file.replace(/\.gguf$/i, "")}
                    </h3>
                    <p className="text-caption text-lagado-red font-medium mt-0.5">
                      {roleOf(m.file, m)}
                    </p>
                  </div>
                  <input
                    type="radio"
                    name="model"
                    checked={isSelected}
                    onChange={() => setSelected(m.file)}
                    className="w-4 h-4 accent-lagado-red mt-1 flex-shrink-0"
                  />
                </div>

                <div className="grid grid-cols-3 gap-2 text-caption mt-4">
                  <div>
                    <span className="text-lagado-text-dim block">Size</span>
                    <span className="font-mono text-lagado-text">{fmtSize(m.size_mb)}</span>
                  </div>
                  <div>
                    <span className="text-lagado-text-dim block">Context</span>
                    <span className="font-mono text-lagado-text">{fmtCtx(m.context_length)}</span>
                  </div>
                  <div>
                    <span className="text-lagado-text-dim block">Type</span>
                    <span className="font-mono text-lagado-text">
                      {m.is_moe ? `MoE ×${m.expert_count}` : "dense"}
                    </span>
                  </div>
                </div>
                <p className={`text-caption font-mono mt-3 ${fit.cls}`}>● {fit.label}</p>
              </div>
            );
          })}
        </div>

        {infra.length > 0 && (
          <p className="text-caption text-lagado-text-dim mb-6">
            Also installed (used automatically):{" "}
            {infra.map((m) => `${m.file.replace(/\.gguf$/i, "")} (${roleOf(m.file, m)})`).join(", ")}
          </p>
        )}

        {error && <p className="text-body-sm text-lagado-red mb-4">{error}</p>}

        <div className="flex gap-3">
          <Button
            variant="primary"
            size="lg"
            onClick={handleContinue}
            disabled={saving || !selected}
            className="flex-1"
          >
            {saving ? "Saving…" : "Continue"}
          </Button>
          <Button variant="secondary" size="lg" className="px-8" onClick={onNext}>
            Skip
          </Button>
        </div>

        <p className="text-caption text-lagado-text-dim mt-4 text-center">
          Models live in <span className="font-mono">~/.laputa-secure/models/</span>
        </p>
      </div>
    </div>
  );
}
