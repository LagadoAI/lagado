import React, { useState } from "react";
import { Card } from "../components/Card";
import { Radio } from "../components/Radio";
import { Select } from "../components/Select";
import { Input } from "../components/Input";
import { Slider } from "../components/Slider";

export default function SettingsInference() {
  const [mode, setMode] = useState("local");
  const [threads, setThreads] = useState(8);
  const [batchSize, setBatchSize] = useState(4);

  return (
    <Card>
      <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Inference Settings</h2>

      <div className="space-y-6">
        <div>
          <label className="block text-body-sm text-lagado-text-dim mb-3">Mode</label>
          <div className="space-y-2">
            <Radio
              name="mode"
              value="local"
              checked={mode === "local"}
              onChange={() => setMode("local")}
              label="Local only"
              description="All processing on your device"
            />
            <Radio
              name="mode"
              value="cloud"
              checked={mode === "cloud"}
              onChange={() => setMode("cloud")}
              label="Cloud"
              description="Use cloud provider (slower, more capable)"
            />
            <Radio
              name="mode"
              value="hybrid"
              checked={mode === "hybrid"}
              onChange={() => setMode("hybrid")}
              label="Hybrid"
              description="Auto-route between local and cloud"
            />
          </div>
        </div>

        {mode !== "local" && (
          <div>
            <label className="block text-body-sm text-lagado-text-dim mb-2">Cloud Provider</label>
            <Select
              value="none"
              onChange={() => {}}
              options={[
                { value: "none", label: "Select provider..." },
                { value: "openai", label: "OpenAI (GPT-4)" },
                { value: "anthropic", label: "Anthropic (Claude)" },
                { value: "google", label: "Google (Gemini)" },
              ]}
            />
            <Input className="mt-3" placeholder="API Key" type="password" />
          </div>
        )}

        <div>
          <Slider
            min={1}
            max={16}
            value={threads}
            onChange={setThreads}
            label={`Threads (${threads})`}
          />
        </div>

        <div>
          <Slider
            min={1}
            max={16}
            value={batchSize}
            onChange={setBatchSize}
            label={`Batch Size (${batchSize})`}
          />
        </div>
      </div>
    </Card>
  );
}
