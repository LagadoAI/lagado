// Code editor with Monaco
 
import React, { useState } from "react";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
import { FileTree } from "../components/FileTree";
 
const sampleFiles = [
  {
    id: "src",
    name: "src",
    type: "folder" as const,
    children: [
      { id: "main", name: "main.ts", type: "file" as const },
      { id: "app", name: "App.tsx", type: "file" as const },
      { id: "test", name: "test.py", type: "file" as const },
    ],
  },
  {
    id: "docs",
    name: "docs",
    type: "folder" as const,
    children: [{ id: "readme", name: "README.md", type: "file" as const }],
  },
];
 
export default function CodePage() {
  const [selectedFile, setSelectedFile] = useState("test.py");
  const [code, setCode] = useState(`# Hello world example
def greet(name):
    print(f"Hello, {name}!")
 
if __name__ == "__main__":
    greet("World")
`);
 
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Code" />
 
      <div className="flex-1 flex overflow-hidden">
        {/* File browser */}
        <div className="w-64 border-r border-lagado-border bg-lagado-surface p-4 overflow-y-auto">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-h3 text-lagado-text-bright font-semibold">Files</h3>
            <button className="text-lagado-text-dim hover:text-lagado-text-bright">+</button>
          </div>
          <FileTree
            files={sampleFiles}
            onSelect={(file) => setSelectedFile(file.name)}
          />
        </div>
 
        {/* Editor area */}
        <div className="flex-1 flex flex-col">
          {/* File tabs */}
          <div className="flex border-b border-lagado-border bg-lagado-surface">
            <div className="px-4 py-2 border-r border-lagado-border bg-lagado-surface-2 text-body-sm text-lagado-text-bright font-mono">
              {selectedFile}
            </div>
          </div>
 
          {/* Editor */}
          <div className="flex-1 p-4 overflow-auto">
            <textarea
              value={code}
              onChange={(e) => setCode(e.target.value)}
              className="w-full h-full bg-lagado-bg text-lagado-text font-mono text-sm p-4 outline-none resize-none border border-lagado-border rounded-sm"
              spellCheck={false}
            />
          </div>
 
          {/* Bottom toolbar */}
          <div className="flex items-center justify-between p-4 border-t border-lagado-border bg-lagado-surface gap-3">
            <div className="flex items-center gap-3">
              <Button variant="primary" size="sm">
                ▶ Test in Sandbox
              </Button>
              <Button variant="secondary" size="sm">
                💾 Save
              </Button>
              <Button variant="secondary" size="sm">
                Format
              </Button>
            </div>
            <div className="text-caption text-lagado-text-dim font-mono">
              Lines: {code.split("\n").length} | Cols: 80
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
