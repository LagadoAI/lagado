 
import React from "react";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
import { ProgressBar } from "../components/ProgressBar";
import { Alert } from "../components/Alert";
 
export default function VaultStorageWarning() {
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Vault - Storage Warning" />
 
      <div className="flex-1 p-6 max-w-3xl mx-auto w-full">
        <Alert
          type="warning"
          title="Vault is 85% full"
          message="Your storage is running low. Consider pruning old files or expanding storage."
        />
 
        <div className="mt-6 bg-lagado-surface border border-lagado-border rounded-sm p-6">
          <h3 className="text-h3 text-lagado-text-bright font-bold mb-4">
            Storage Usage
          </h3>
          <ProgressBar value={8.5} max={10} label="" showPercent />
          <div className="grid grid-cols-2 gap-4 mt-6">
            <div>
              <p className="text-caption text-lagado-text-dim">Used</p>
              <p className="text-h2 text-lagado-text-bright font-bold">8.5 GB</p>
            </div>
            <div>
              <p className="text-caption text-lagado-text-dim">Total</p>
              <p className="text-h2 text-lagado-text-bright font-bold">10 GB</p>
            </div>
          </div>
        </div>
 
        <div className="mt-6 grid grid-cols-1 md:grid-cols-3 gap-3">
          <Button variant="primary" size="lg">Prune Old Files</Button>
          <Button variant="secondary" size="lg">Expand Storage</Button>
          <Button variant="secondary" size="lg">View Largest Files</Button>
        </div>
      </div>
    </div>
  );
}
