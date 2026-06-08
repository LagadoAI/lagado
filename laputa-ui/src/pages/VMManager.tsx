 
export default function VMManager() {
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="VM Manager" />
 
      <div className="flex-1 p-6 max-w-3xl mx-auto w-full">
        <Card className="mb-4">
          <h2 className="text-h2 text-lagado-text-bright font-bold mb-4">Current VM</h2>
 
          <div className="bg-lagado-surface-2 border border-lagado-border rounded-sm p-4">
            <div className="flex items-center justify-between mb-3">
              <span className="text-body text-lagado-text-bright font-semibold">
                Arch Linux XFCE
              </span>
              <Badge variant="success">RUNNING</Badge>
            </div>
            <MetadataList
              items={[
                { key: "CPU", value: "2 cores" },
                { key: "RAM", value: "2 GB" },
                { key: "Storage", value: "8 GB" },
              ]}
            />
            <div className="flex gap-2 mt-4">
              <Button variant="primary" size="sm">Pause</Button>
              <Button variant="secondary" size="sm">Restart</Button>
              <Button variant="danger" size="sm">Stop</Button>
            </div>
          </div>
        </Card>
 
        <Card className="mb-4">
          <h3 className="text-h3 text-lagado-text-bright font-bold mb-4">Load New OS</h3>
          <Button variant="primary" size="md">Select ISO File</Button>
          <div className="mt-4">
            <p className="text-body-sm text-lagado-text-dim mb-2">Available ISOs:</p>
            <ul className="text-body-sm space-y-1 ml-4">
              <li>• Ubuntu 22.04</li>
              <li>• Windows 11 (custom)</li>
              <li>• macOS (limited)</li>
            </ul>
            <Button variant="secondary" size="sm" className="mt-3">+ Add ISO</Button>
          </div>
        </Card>
 
        <Card>
          <h3 className="text-h3 text-lagado-text-bright font-bold mb-4">Snapshots</h3>
          <div className="space-y-2">
            <div className="flex justify-between items-center p-3 bg-lagado-surface-2 border border-lagado-border rounded-sm">
              <span className="text-body-sm">Default state - 5/26 10:00</span>
              <Button variant="secondary" size="sm">Restore</Button>
            </div>
            <div className="flex justify-between items-center p-3 bg-lagado-surface-2 border border-lagado-border rounded-sm">
              <span className="text-body-sm">After Firefox install - 5/26 14:30</span>
              <Button variant="secondary" size="sm">Restore</Button>
            </div>
          </div>
          <Button variant="primary" size="md" className="mt-4">Create Snapshot</Button>
        </Card>
      </div>
    </div>
  );
}
