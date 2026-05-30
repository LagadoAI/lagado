 
export default function MCPAddTool() {
  return (
    <div className="min-h-screen bg-laputa-bg flex flex-col">
      <Header title="MCP - Add Tool" />
 
      <div className="flex-1 p-6 max-w-2xl mx-auto w-full">
        <Card>
          <h2 className="text-h2 text-laputa-text-bright font-bold mb-6">Add MCP Tool</h2>
 
          <div className="space-y-6">
            <div>
              <label className="block text-body-sm text-laputa-text-dim mb-2">
                Tool URL
              </label>
              <Input placeholder="https://example.com/mcp/server" />
            </div>
 
            <div>
              <label className="block text-body-sm text-laputa-text-dim mb-2">
                Or browse from
              </label>
              <div className="flex gap-3">
                <Button variant="secondary" size="md">Marketplace</Button>
                <Button variant="secondary" size="md">Local Path</Button>
              </div>
            </div>
 
            <div className="pt-4 border-t border-laputa-border flex gap-3">
              <Button variant="secondary" size="md" className="flex-1">Cancel</Button>
              <Button variant="primary" size="md" className="flex-1">Add Tool</Button>
            </div>
          </div>
        </Card>
      </div>
    </div>
  );
}
