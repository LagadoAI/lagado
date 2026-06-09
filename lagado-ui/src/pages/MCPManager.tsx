 
export default function MCPManager() {
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="MCP Manager" />
 
      <div className="flex-1 p-6 max-w-5xl mx-auto w-full">
        <div className="flex justify-between items-center mb-6">
          <div>
            <h1 className="text-h1 text-lagado-text-bright font-bold">MCP Manager</h1>
            <p className="text-body text-lagado-text-dim mt-1">
              Connected Tools (12)
            </p>
          </div>
          <Button variant="primary" size="md">+ Add Tool</Button>
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
