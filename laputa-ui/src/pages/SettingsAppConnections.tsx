 
function SettingsAppConnections() {
  const apps = [
    { name: "Email (IMAP/SMTP)", status: "disconnected", icon: "📧" },
    { name: "Calendar (CalDAV)", status: "disconnected", icon: "📅" },
    { name: "Cloud Storage", status: "disconnected", icon: "☁" },
  ];
 
  return (
    <Card>
      <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">External Apps</h2>
 
      <div className="space-y-3">
        {apps.map((app) => (
          <div
            key={app.name}
            className="flex items-center justify-between p-4 bg-lagado-surface-2 border border-lagado-border rounded-sm"
          >
            <div className="flex items-center gap-3">
              <span className="text-2xl">{app.icon}</span>
              <div>
                <div className="text-body text-lagado-text-bright font-semibold">{app.name}</div>
                <div className="text-caption text-lagado-text-dim">
                  Status: {app.status === "connected" ? "✓ Connected" : "○ Not configured"}
                </div>
              </div>
            </div>
            <Button variant="secondary" size="sm">
              {app.status === "connected" ? "Disconnect" : "Configure"}
            </Button>
          </div>
        ))}
      </div>
 
      <Alert
        type="info"
        title="More integrations available"
        message="Most app integrations are available through the MCP marketplace."
        className="mt-6"
      />
      <Button variant="primary" size="md" className="mt-4">
        Go to MCP Manager
      </Button>
    </Card>
  );
}
