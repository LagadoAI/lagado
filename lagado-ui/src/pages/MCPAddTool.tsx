
import { useNavigate } from "react-router-dom";
import { Header } from "../components/Header";
import { Card } from "../components/Card";
import { Input } from "../components/Input";
import { Button } from "../components/Button";

export default function MCPAddTool() {
  const navigate = useNavigate();
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="MCP - Add Tool" />

      <div className="border-b border-lagado-border bg-lagado-surface px-4 py-3">
        <button onClick={() => navigate("/mcp")} className="px-3 py-1.5 text-body-sm text-lagado-text-dim hover:text-lagado-text border border-lagado-border rounded-md hover:border-lagado-red transition-colors">
          ← Back
        </button>
      </div>

      <div className="flex-1 p-6 max-w-2xl mx-auto w-full">
        <Card>
          <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Add MCP Tool</h2>
 
          <div className="space-y-6">
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                Tool URL
              </label>
              <Input placeholder="https://example.com/mcp/server" />
            </div>
 
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                Or browse from
              </label>
              <div className="flex gap-3">
                <Button variant="secondary" size="md">Marketplace</Button>
                <Button variant="secondary" size="md">Local Path</Button>
              </div>
            </div>
 
            <div className="pt-4 border-t border-lagado-border flex gap-3">
              <Button variant="secondary" size="md" className="flex-1">Cancel</Button>
              <Button variant="primary" size="md" className="flex-1">Add Tool</Button>
            </div>
          </div>
        </Card>
      </div>
    </div>
  );
}
