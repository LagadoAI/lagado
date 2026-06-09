 
import React, { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { MetadataList } from "../components/MetadataList";
 
interface FirstLaunchSystemDetectedProps {
  onNext: () => void;
}
 
interface SystemInfo {
  cpu: string;
  cores: number;
  threads: number;
  ram: string;
  gpu: string;
  gpuMemory: string;
  storage: string;
  os: string;
}
 
export default function FirstLaunchSystemDetected({ onNext }: FirstLaunchSystemDetectedProps) {
  const navigate = useNavigate();
  const [isDetecting, setIsDetecting] = useState(true);
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);
 
  useEffect(() => {
    // Simulate system detection
    setTimeout(() => {
      setSystemInfo({
        cpu: "Intel i7-9750H",
        cores: 6,
        threads: 12,
        ram: "16 GB",
        gpu: "NVIDIA RTX 3060",
        gpuMemory: "6 GB",
        storage: "256 GB free of 512 GB",
        os: "CachyOS Linux (Arch)",
      });
      setIsDetecting(false);
    }, 2000);
  }, []);
 
  const handleNext = () => {
    onNext();
    navigate("/setup/models");
  };
 
  if (isDetecting) {
    return (
      <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
        <div className="text-center">
          <Spinner size="lg" label="Detecting your system..." />
          <p className="mt-6 text-body-sm text-lagado-text-dim max-w-sm">
            Reading hardware specs to recommend the best configuration
          </p>
        </div>
      </div>
    );
  }
 
  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
      <div className="max-w-2xl w-full">
        <h1 className="text-h1 text-lagado-text-bright font-bold mb-2">
          System Detected
        </h1>
        <p className="text-body text-lagado-text-dim mb-8">
          Here's what we found on your machine
        </p>
 
        <div className="bg-lagado-surface border border-lagado-border rounded-sm p-6 mb-6">
          <MetadataList
            items={[
              { key: "CPU", value: systemInfo!.cpu, mono: true },
              {
                key: "Cores / Threads",
                value: `${systemInfo!.cores} / ${systemInfo!.threads}`,
                mono: true,
              },
              { key: "RAM", value: systemInfo!.ram, mono: true },
              { key: "GPU", value: systemInfo!.gpu, mono: true },
              { key: "GPU Memory", value: systemInfo!.gpuMemory, mono: true },
              { key: "Storage", value: systemInfo!.storage, mono: true },
              { key: "Operating System", value: systemInfo!.os, mono: true },
            ]}
          />
        </div>
 
        <div className="bg-lagado-purple bg-opacity-10 border border-lagado-purple rounded-sm p-4 mb-8">
          <div className="flex items-start gap-3">
            <span className="text-2xl">💡</span>
            <div>
              <p className="text-body text-lagado-purple font-semibold mb-1">
                Recommendation
              </p>
              <p className="text-body-sm text-lagado-text">
                Your system can run a balanced model. We'll suggest{" "}
                <span className="font-mono">LFM2.5-8B</span> on the next step.
              </p>
            </div>
          </div>
        </div>
 
        <div className="flex gap-3">
          <Button variant="primary" size="lg" onClick={handleNext} className="flex-1">
            Continue
          </Button>
          <Button variant="secondary" size="lg" className="flex-1">
            Manual Setup
          </Button>
        </div>
      </div>
    </div>
  );
}
 
