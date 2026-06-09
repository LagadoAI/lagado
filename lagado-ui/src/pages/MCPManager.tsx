
import { useNavigate } from "react-router-dom";
import { Header } from "../components/Header";
import { Button } from "../components/Button";
import { Card } from "../components/Card";

export default function MCPManager() {
  const navigate = useNavigate();
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="MCP Manager" />

      <div className="border-b border-lagado-border bg-lagado-surface px-4 py-3">
        <button onClick={() => navigate("/chat")} className="px-3 py-1.5 text-body-sm text-lagado-text-dim hover:text-lagado-text border border-lagado-border rounded-md hover:border-lagado-red transition-colors">
          ← Chat
        </button>
      </div>

      {/* Phase 2 coming soon banner */}
      <div className="bg-lagado-blue bg-opacity-10 border-b border-lagado-blue border-opacity-30 px-4 py-3">
        <div className="flex items-center gap-2 max-w-3xl">
          <span className="text-lagado-blue text-sm">◆</span>
          <span className="text-body-sm text-lagado-text">
            This feature is coming in Phase 2. The interface is a preview.
          </span>
        </div>
      </div>

      <div className="flex-1 p-6 max-w-5xl mx-auto w-full">
        <div className="flex justify-between items-center mb-6">
          <div>
            <h1 className="text-h1 text-lagado-text-bright font-bold">MCP Manager</h1>
            <p className="text-body text-lagado-text-dim mt-1">
              Connected Tools (12)
            </p>
          </div>
          <Button variant="primary" size="md" onClick={() => navigate('/mcp/add')}>+ Add Tool</Button>
        </div>
 
        <div className="space-y-4">
          {[
            { name: "filesystem-mcp v1.2.3", desc: "Filesystem operations" },
            { name: "browser-mcp v2.1.0", desc: "Browser automation" },
            { name: "github-mcp v1.0.5", desc: "GitHub integration" },
            { name: "slack-mcp v1.4.2", desc: "Slack messaging" },
          ].map((tool, idx) => (
            <Card key={idx}>
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <span className="text-lagado-green text-xl">●</span>
                  <div>
                    <div className="text-body text-lagado-text-bright font-semibold">
                      {tool.name}
                    </div>
                    <div className="text-body-sm text-lagado-text-dim">
                      {tool.desc} • Status: Active
                    </div>
                  </div>
                </div>
                <div className="flex gap-2">
                  <Button variant="secondary" size="sm">Enable</Button>
                  <Button variant="secondary" size="sm">Disable</Button>
                  <Button variant="danger" size="sm">Remove</Button>
                </div>
              </div>
            </Card>
          ))}
        </div>
 
        <div className="mt-6 text-center">
          <Button variant="secondary" size="md">Search Marketplace</Button>
        </div>
      </div>
    </div>
  );
}
