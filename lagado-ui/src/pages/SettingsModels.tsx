import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Card } from "../components/Card";
import { Button } from "../components/Button";
import { Checkbox } from "../components/Checkbox";
import { Badge } from "../components/Badge";

function SettingsModels() {
  const [models, setModels] = useState<string[]>([]);
  const [activeModel, setActiveModelState] = useState<string>("");
  const [timeline, setTimeline] = useState<Array<{ timestamp: number; active_goal: string; last_action: string }>>([]);
  const [autoSwitch, setAutoSwitch] = useState(false);

  useEffect(() => {
    invoke<string[]>("list_models")
      .then(setModels)
      .catch(() => {});
    invoke<string>("get_active_model")
      .then(setActiveModelState)
      .catch(() => {});
    invoke<any[]>("get_chronos_recent", { n: 10 })
      .then(setTimeline)
      .catch(() => {});
  }, []);

  const handleModelChange = (filename: string) => {
    invoke("set_active_model", { filename })
      .then(() => setActiveModelState(filename))
      .catch(console.error);
  };
 
  return (
    <div className="space-y-8">
      <Card>
        <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Active Model</h2>
        <div className="space-y-2">
          {models.length === 0 ? (
            <p className="text-lagado-text-dim text-body-sm">No models found in models directory.</p>
          ) : (
            models.map((m) => (
              <button
                key={m}
                onClick={() => handleModelChange(m)}
                className={`w-full text-left px-4 py-3 rounded-md border text-body-sm transition-colors ${
                  m === activeModel
                    ? "border-lagado-red bg-lagado-red bg-opacity-10 text-lagado-text-bright"
                    : "border-lagado-border text-lagado-text hover:border-lagado-red"
                }`}
              >
                {m} {m === activeModel && <span className="text-lagado-red ml-2">● active</span>}
              </button>
            ))
          )}
        </div>
      </Card>

      <Card>
        <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Recent Timeline</h2>
        {timeline.length === 0 ? (
          <p className="text-lagado-text-dim text-body-sm">No timeline entries yet.</p>
        ) : (
          <div className="space-y-2">
            {timeline.map((entry, i) => (
              <div key={i} className="px-4 py-2 bg-lagado-surface-2 rounded-md border border-lagado-border">
                <p className="text-body-sm text-lagado-text-bright truncate">{entry.active_goal}</p>
                <p className="text-caption text-lagado-text-dim">{entry.last_action}</p>
                <p className="text-caption text-lagado-text-dim opacity-50">
                  {new Date(entry.timestamp * 1000).toLocaleString()}
                </p>
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
}
