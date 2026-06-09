 
import React from "react";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
import { MetadataList } from "../components/MetadataList";
 
export default function VaultFilePreview() {
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Vault > project_brief.md" />
 
      <div className="flex-1 flex">
        {/* Sidebar */}
        <div className="w-64 border-r border-lagado-border bg-lagado-surface p-4">
          <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">Files</h3>
          <div className="space-y-1 text-body-sm">
            <div className="px-2 py-1 hover:bg-lagado-surface-2 rounded cursor-pointer">📁 Docs/</div>
            <div className="px-2 py-1 bg-lagado-surface-2 rounded cursor-pointer text-lagado-text-bright">
              📄 project_brief.md
            </div>
          </div>
        </div>
 
        {/* Preview area */}
        <div className="flex-1 flex flex-col">
          <div className="flex-1 p-6 overflow-y-auto">
            <div className="max-w-3xl mx-auto">
              <h1 className="text-h1 text-lagado-text-bright font-bold mb-4">
                Project Brief
              </h1>
              <p className="text-body text-lagado-text mb-4">
                This is a markdown document showing the preview of the file in the vault.
                Files are sanitized before display - no scripts, no external resources.
              </p>
              <h2 className="text-h2 text-lagado-text-bright font-bold mb-3 mt-6">
                Section 1
              </h2>
              <p className="text-body text-lagado-text mb-3">
                Content here...
              </p>
            </div>
          </div>
 
          {/* Footer with actions */}
          <div className="p-4 border-t border-lagado-border bg-lagado-surface flex items-center justify-between">
            <MetadataList
              items={[
                { key: "Size", value: "2.3 KB" },
                { key: "Modified", value: "Today" },
                { key: "Tags", value: "project, brief" },
              ]}
              className="flex gap-6"
            />
            <div className="flex gap-2">
              <Button variant="secondary" size="sm">Open in Code</Button>
              <Button variant="secondary" size="sm">Download</Button>
              <Button variant="primary" size="sm">Add to Chat</Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
