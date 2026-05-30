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
    <div className="min-h-screen bg-laputa-bg flex flex-col">
      <Header title="Code" />
 
      <div className="flex-1 flex overflow-hidden">
        {/* File browser */}
        <div className="w-64 border-r border-laputa-border bg-laputa-surface p-4 overflow-y-auto">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-h3 text-laputa-text-bright font-semibold">Files</h3>
            <button className="text-laputa-text-dim hover:text-laputa-text-bright">+</button>
          </div>
          <FileTree
            files={sampleFiles}
            onSelect={(file) => setSelectedFile(file.name)}
          />
        </div>
 
        {/* Editor area */}
        <div className="flex-1 flex flex-col">
          {/* File tabs */}
          <div className="flex border-b border-laputa-border bg-laputa-surface">
            <div className="px-4 py-2 border-r border-laputa-border bg-laputa-surface-2 text-body-sm text-laputa-text-bright font-mono">
              {selectedFile}
            </div>
          </div>
 
          {/* Editor */}
          <div className="flex-1 p-4 overflow-auto">
            <textarea
              value={code}
              onChange={(e) => setCode(e.target.value)}
              className="w-full h-full bg-laputa-bg text-laputa-text font-mono text-sm p-4 outline-none resize-none border border-laputa-border rounded-sm"
              spellCheck={false}
            />
          </div>
 
          {/* Bottom toolbar */}
          <div className="flex items-center justify-between p-4 border-t border-laputa-border bg-laputa-surface gap-3">
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
            <div className="text-caption text-laputa-text-dim font-mono">
              Lines: {code.split("\n").length} | Cols: 80
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
