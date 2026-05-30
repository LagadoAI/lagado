 
import React, { useState } from "react";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
import { ProgressBar } from "../components/ProgressBar";
 
interface VaultFile {
  id: string;
  name: string;
  size: string;
  modified: Date;
  type: "doc" | "image" | "code" | "archive";
}
 
export default function VaultDefault() {
  const [selectedFolder, setSelectedFolder] = useState("documents");
  const [searchTerm, setSearchTerm] = useState("");
 
  const folders = [
    { id: "documents", name: "Documents", icon: "📄" },
    { id: "images", name: "Images", icon: "🖼" },
    { id: "code", name: "Code", icon: "💻" },
    { id: "archives", name: "Archives", icon: "📦" },
  ];
 
  const files: VaultFile[] = [
    { id: "1", name: "project_brief.md", size: "2.3 KB", modified: new Date(), type: "doc" },
    { id: "2", name: "presentation.pdf", size: "1.2 MB", modified: new Date(Date.now() - 86400000), type: "doc" },
    { id: "3", name: "design_notes.png", size: "450 KB", modified: new Date(Date.now() - 86400000 * 3), type: "image" },
  ];
 
  return (
    <div className="min-h-screen bg-laputa-bg flex flex-col">
      <Header title="Vault" />
 
      <div className="flex-1 flex">
        {/* Sidebar */}
        <div className="w-64 border-r border-laputa-border bg-laputa-surface p-4">
          <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">Folders</h3>
          <div className="space-y-1">
            {folders.map((folder) => (
              <button
                key={folder.id}
                onClick={() => setSelectedFolder(folder.id)}
                className={`w-full text-left px-3 py-2 rounded-sm flex items-center gap-2 transition-colors ${
                  selectedFolder === folder.id
                    ? "bg-laputa-surface-2 text-laputa-text-bright"
                    : "text-laputa-text hover:bg-laputa-surface-2"
                }`}
              >
                <span>{folder.icon}</span>
                <span className="text-body-sm">{folder.name}</span>
              </button>
            ))}
          </div>
          <Button variant="secondary" size="sm" className="w-full mt-4">
            + Add Folder
          </Button>
        </div>
 
        {/* Main content */}
        <div className="flex-1 flex flex-col">
          {/* Search bar */}
          <div className="p-4 border-b border-laputa-border">
            <input
              type="text"
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              placeholder="Search files..."
              className="w-full px-3 py-2 rounded-sm bg-laputa-surface-2 border border-laputa-border text-laputa-text focus:border-laputa-red focus:outline-none font-rajdhani text-body"
            />
          </div>
 
          {/* File list */}
          <div className="flex-1 overflow-y-auto p-4">
            <div className="space-y-2">
              {files.map((file) => (
                <div
                  key={file.id}
                  className="flex items-center justify-between p-3 bg-laputa-surface border border-laputa-border rounded-sm hover:border-laputa-border-light transition-colors cursor-pointer"
                >
                  <div className="flex items-center gap-3">
                    <span className="text-2xl">
                      {file.type === "doc" ? "📄" : file.type === "image" ? "🖼" : "📦"}
                    </span>
                    <div>
                      <div className="text-body text-laputa-text-bright">{file.name}</div>
                      <div className="text-caption text-laputa-text-dim">
                        {file.size} • {file.modified.toLocaleDateString()}
                      </div>
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <button className="text-laputa-text-dim hover:text-laputa-text">👁</button>
                    <button className="text-laputa-text-dim hover:text-laputa-text">⬇</button>
                  </div>
                </div>
              ))}
            </div>
          </div>
 
          {/* Storage indicator */}
          <div className="p-4 border-t border-laputa-border bg-laputa-surface">
            <ProgressBar
              value={5.2}
              max={10}
              label="Storage"
              showPercent
            />
            <p className="text-caption text-laputa-text-dim mt-2 text-center">
              5.2 GB used of 10 GB
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
