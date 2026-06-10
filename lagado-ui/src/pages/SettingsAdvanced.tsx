import React, { useState } from "react";
import { Card } from "../components/Card";
import { Radio } from "../components/Radio";
import { Input } from "../components/Input";
import { Select } from "../components/Select";

export default function SettingsAdvanced() {
  const [mode, setMode] = useState("proven");
 
  return (
    <Card>
      <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Advanced</h2>
 
      <div className="space-y-6">
        <div>
          <label className="block text-body-sm text-lagado-text-dim mb-3">Mode</label>
          <div className="space-y-2">
            <Radio
              name="mode"
              value="proven"
              checked={mode === "proven"}
              onChange={() => setMode("proven")}
              label="Proven defaults"
              description="Use battle-tested configurations"
            />
            <Radio
              name="mode"
              value="tune"
              checked={mode === "tune"}
              onChange={() => setMode("tune")}
              label="I want to tune"
              description="⚠ Advanced - You'll need to understand each setting"
            />
          </div>
        </div>
 
        {mode === "tune" && (
          <>
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                Sampling Parameters
              </label>
              <div className="grid grid-cols-3 gap-3">
                <div>
                  <label className="text-caption text-lagado-text-dim">temp</label>
                  <Input type="number" defaultValue="0.7" />
                </div>
                <div>
                  <label className="text-caption text-lagado-text-dim">top_p</label>
                  <Input type="number" defaultValue="0.9" />
                </div>
                <div>
                  <label className="text-caption text-lagado-text-dim">top_k</label>
                  <Input type="number" defaultValue="40" />
                </div>
              </div>
            </div>
 
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                llama.cpp Flags
              </label>
              <Input placeholder="--ctx-size 4096 --batch-size 512" />
            </div>
 
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                Context Strategy
              </label>
              <Select
                value="trim"
                onChange={() => {}}
                options={[
                  { value: "trim", label: "Trim from start" },
                  { value: "summarize", label: "Summarize history" },
                  { value: "rolling", label: "Rolling window" },
                ]}
              />
            </div>
 
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                Quantization
              </label>
              <Select
                value="iq4"
                onChange={() => {}}
                options={[
                  { value: "iq4", label: "IQ4 (Recommended)" },
                  { value: "q4_k", label: "Q4_K" },
                  { value: "q5_k", label: "Q5_K" },
                  { value: "q6_k", label: "Q6_K" },
                  { value: "q8_0", label: "Q8_0" },
                ]}
              />
            </div>
          </>
        )}
 
        <div>
          <label className="block text-body-sm text-lagado-text-dim mb-2">Logging Level</label>
          <Select
            value="info"
            onChange={() => {}}
            options={[
              { value: "error", label: "Error only" },
              { value: "warn", label: "Warning" },
              { value: "info", label: "Info" },
              { value: "debug", label: "Debug" },
              { value: "trace", label: "Trace (verbose)" },
            ]}
          />
        </div>
      </div>
    </Card>
  );
}
