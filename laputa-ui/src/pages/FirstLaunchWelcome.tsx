 
import React from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "../components/Button";
 
interface FirstLaunchWelcomeProps {
  onNext: () => void;
}
 
export default function FirstLaunchWelcome({ onNext }: FirstLaunchWelcomeProps) {
  const navigate = useNavigate();
 
  const handleNext = () => {
    onNext();
    navigate("/setup/system");
  };
 
  return (
    <div className="min-h-screen bg-laputa-bg flex items-center justify-center px-4">
      <div className="max-w-2xl w-full text-center">
        {/* Logo */}
        <div className="inline-block w-32 h-32 bg-laputa-purple-mid rounded-lg mb-8 flex items-center justify-center border border-laputa-purple">
          <span className="text-6xl">⚔</span>
        </div>
 
        <h1 className="text-h1 text-laputa-text-bright font-bold tracking-wider mb-4">
          Welcome to Laputa
        </h1>
 
        <p className="text-body text-laputa-text mb-2">
          Your local AI agent
        </p>
 
        <p className="text-body-sm text-laputa-text-dim mb-12">
          Local • Private • Encrypted • Yours
        </p>
 
        {/* Steps Preview */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-12">
          <div className="bg-laputa-surface border border-laputa-border rounded-sm p-4 text-left">
            <div className="text-laputa-purple text-2xl mb-2">🔍</div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-1">
              Detect System
            </h3>
            <p className="text-body-sm text-laputa-text-dim">
              We'll analyze your hardware
            </p>
          </div>
 
          <div className="bg-laputa-surface border border-laputa-border rounded-sm p-4 text-left">
            <div className="text-laputa-purple text-2xl mb-2">⚡</div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-1">
              Choose Model
            </h3>
            <p className="text-body-sm text-laputa-text-dim">
              Pick the right AI for you
            </p>
          </div>
 
          <div className="bg-laputa-surface border border-laputa-border rounded-sm p-4 text-left">
            <div className="text-laputa-purple text-2xl mb-2">🔒</div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-1">
              Set Permissions
            </h3>
            <p className="text-body-sm text-laputa-text-dim">
              Choose what Laputa can access
            </p>
          </div>
 
          <div className="bg-laputa-surface border border-laputa-border rounded-sm p-4 text-left">
            <div className="text-laputa-purple text-2xl mb-2">🚀</div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-1">
              Start Using
            </h3>
            <p className="text-body-sm text-laputa-text-dim">
              Begin your AI journey
            </p>
          </div>
        </div>
 
        <Button variant="primary" size="lg" onClick={handleNext}>
          Get Started
        </Button>
 
        <p className="mt-8 text-caption text-laputa-text-dim">
          Setup takes ~3 minutes • Your data never leaves your device
        </p>
      </div>
    </div>
  );
}
