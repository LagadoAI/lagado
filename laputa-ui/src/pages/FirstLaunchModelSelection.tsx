 
import React, { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "../components/Button";
import { Radio } from "../components/Radio";
import { Badge } from "../components/Badge";
 
interface FirstLaunchModelSelectionProps {
  onNext: () => void;
}
 
const models = [
  {
    id: "tinyllama-1b",
    name: "TinyLlama-1B",
    description: "Lightweight, CPU-friendly",
    size: "800 MB",
    ram: "1.5 GB",
    speed: "Very Fast",
    quality: "Basic",
    tier: "lightweight",
  },
  {
    id: "qwen-2.5b",
    name: "Qwen3-2.5B",
    description: "Balanced performance",
    size: "1.8 GB",
    ram: "3 GB",
    speed: "Fast",
    quality: "Good",
    tier: "balanced",
    recommended: true,
  },
  {
    id: "qwen-8b",
    name: "Qwen3-8B",
    description: "Maximum quality",
    size: "5.5 GB",
    ram: "8 GB",
    speed: "Slower",
    quality: "Excellent",
    tier: "powerful",
    requiresGPU: true,
  },
];
 
export default function FirstLaunchModelSelection({
  onNext,
}: FirstLaunchModelSelectionProps) {
  const navigate = useNavigate();
  const [selectedModel, setSelectedModel] = useState("qwen-2.5b");
 
  const handleNext = () => {
    onNext();
    navigate("/setup/permissions");
  };
 
  return (
    <div className="min-h-screen bg-laputa-bg flex items-center justify-center px-4">
      <div className="max-w-3xl w-full">
        <h1 className="text-h1 text-laputa-text-bright font-bold mb-2">
          Choose Your Model
        </h1>
        <p className="text-body text-laputa-text-dim mb-8">
          You can change this later in Settings
        </p>
 
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-8">
          {models.map((model) => (
            <div
              key={model.id}
              onClick={() => setSelectedModel(model.id)}
              className={`
                p-6 border rounded-sm cursor-pointer transition-all
                ${
                  selectedModel === model.id
                    ? "border-laputa-red bg-laputa-red-dim"
                    : "border-laputa-border bg-laputa-surface hover:border-laputa-border-light"
                }
              `}
            >
              {/* Recommended Badge */}
              {model.recommended && (
                <div className="mb-3">
                  <Badge variant="success">RECOMMENDED</Badge>
                </div>
              )}
 
              <h3 className="text-h3 text-laputa-text-bright font-bold mb-1">
                {model.name}
              </h3>
              <p className="text-body-sm text-laputa-text-dim mb-4">
                {model.description}
              </p>
 
              {/* Stats */}
              <div className="space-y-2 text-body-sm">
                <div className="flex justify-between">
                  <span className="text-laputa-text-dim">Size:</span>
                  <span className="font-mono">{model.size}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-laputa-text-dim">RAM:</span>
                  <span className="font-mono">{model.ram}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-laputa-text-dim">Speed:</span>
                  <span className="font-mono">{model.speed}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-laputa-text-dim">Quality:</span>
                  <span className="font-mono">{model.quality}</span>
                </div>
              </div>
 
              {model.requiresGPU && (
                <div className="mt-3 pt-3 border-t border-laputa-border">
                  <Badge variant="warning">GPU REQUIRED</Badge>
                </div>
              )}
 
              <div className="mt-4 flex items-center justify-center">
                <input
                  type="radio"
                  name="model"
                  value={model.id}
                  checked={selectedModel === model.id}
                  onChange={() => setSelectedModel(model.id)}
                  className="w-5 h-5 accent-laputa-red"
                />
              </div>
            </div>
          ))}
        </div>
 
        <div className="flex gap-3">
          <Button variant="primary" size="lg" onClick={handleNext} className="flex-1">
            Continue
          </Button>
          <Button variant="secondary" size="lg" className="flex-1">
            Custom Model
          </Button>
        </div>
      </div>
    </div>
  );
}
