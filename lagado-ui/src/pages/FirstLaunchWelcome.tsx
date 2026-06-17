import React from "react";
import { useNavigate } from "react-router-dom";
import { Cpu, Boxes, ShieldCheck, Rocket } from "lucide-react";
import { Button } from "../components/Button";

interface FirstLaunchWelcomeProps {
  onNext: () => void;
}

const STEPS = [
  { icon: Cpu, title: "Detect System", desc: "We read your real hardware" },
  { icon: Boxes, title: "Choose Model", desc: "Pick the right local brain" },
  { icon: ShieldCheck, title: "Set Permissions", desc: "Choose what Lagado can access" },
  { icon: Rocket, title: "Start Using", desc: "Begin — fully local" },
];

export default function FirstLaunchWelcome({ onNext }: FirstLaunchWelcomeProps) {
  const navigate = useNavigate();

  const handleNext = () => {
    onNext();
    navigate("/setup/system");
  };

  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4 py-10">
      <div className="max-w-2xl w-full text-center">
        <div className="inline-flex w-28 h-28 bg-lagado-purple-mid rounded-2xl mb-8 items-center justify-center border border-lagado-purple">
          <span className="text-5xl">⚔</span>
        </div>

        <h1 className="text-h1 text-lagado-text-bright font-bold tracking-wider mb-4">
          Welcome to Lagado
        </h1>
        <p className="text-body text-lagado-text mb-2">Your local AI agent</p>
        <p className="text-body-sm text-lagado-text-dim mb-12">
          Local • Private • Encrypted • Yours
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-12">
          {STEPS.map(({ icon: Icon, title, desc }) => (
            <div
              key={title}
              className="bg-lagado-surface border border-lagado-border rounded-md p-4 text-left"
            >
              <Icon size={22} className="text-lagado-purple mb-2" />
              <h3 className="text-h3 text-lagado-text-bright font-semibold mb-1">{title}</h3>
              <p className="text-body-sm text-lagado-text-dim">{desc}</p>
            </div>
          ))}
        </div>

        <Button variant="primary" size="lg" onClick={handleNext}>
          Get Started
        </Button>

        <p className="mt-8 text-caption text-lagado-text-dim">
          Setup takes ~3 minutes • Your data never leaves your device
        </p>
      </div>
    </div>
  );
}
