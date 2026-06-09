
import React, { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from '@tauri-apps/api/core';
import { Header } from "../components/Header";
import { Button } from "../components/Button";
import { ProgressBar } from "../components/ProgressBar";
 
interface VaultFile {
  name: string;
  is_dir: boolean;
  size: string;
}
 
export default function VaultDefault() {
  const navigate = useNavigate();
  const [currentPath, setCurrentPath] = useState("");
  const [realFiles, setRealFiles] = useState<VaultFile[]>([]);
  const [searchTerm, setSearchTerm] = useState("");

  useEffect(() => {
    invoke<Array<{ name: string; is_dir: boolean; size: string }>>(
      "vault_list_files",
      { subfolder: currentPath }
    )
      .then(setRealFiles)
      .catch(() => setRealFiles([]));
  }, [currentPath]);
 
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Vault" />

      <div className="border-b border-lagado-border bg-lagado-surface px-4 py-3">
        <button onClick={() => navigate("/chat")} className="px-3 py-1.5 text-body-sm text-lagado-text-dim hover:text-lagado-text border border-lagado-border rounded-md hover:border-lagado-red transition-colors">
          ← Chat
        </button>
      </div>

      <div className="flex-1 flex">
        {/* Sidebar */}
        <div className="w-64 border-r border-lagado-border bg-lagado-surface p-4">
          <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">Path</h3>
          <div className="text-body-sm text-lagado-text mb-4">
            {currentPath || "~/.laputa-secure"}
          </div>
        </div>
 
        {/* Main content */}
        <div className="flex-1 flex flex-col">
          {/* Search bar */}
          <div className="p-4 border-b border-lagado-border">
            <input
              type="text"
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              placeholder="Search files..."
              className="w-full px-3 py-2 rounded-sm bg-lagado-surface-2 border border-lagado-border text-lagado-text focus:border-lagado-red focus:outline-none font-rajdhani text-body"
            />
          </div>
 
          {/* File list */}
          <div className="flex-1 overflow-y-auto p-4">
            <div className="space-y-2">
              {realFiles.map((file) => (
                <div
                  key={file.name}
                  className="flex items-center justify-between p-3 bg-lagado-surface border border-lagado-border rounded-sm hover:border-lagado-border-light transition-colors cursor-pointer"
                >
                  <div className="flex items-center gap-3">
                    <span className="text-2xl">
                      {file.is_dir ? "📁" : "📄"}
                    </span>
                    <div>
                      <div className="text-body text-lagado-text-bright">{file.name}</div>
                      <div className="text-caption text-lagado-text-dim">
                        {file.size}
                      </div>
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <button className="text-lagado-text-dim hover:text-lagado-text">👁</button>
                    <button className="text-lagado-text-dim hover:text-lagado-text">⬇</button>
                  </div>
                </div>
              ))}
            </div>
          </div>
 
          {/* Storage indicator */}
          <div className="p-4 border-t border-lagado-border bg-lagado-surface">
            <ProgressBar
              value={5.2}
              max={10}
              label="Storage"
              showPercent
            />
            <p className="text-caption text-lagado-text-dim mt-2 text-center">
              5.2 GB used of 10 GB
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
