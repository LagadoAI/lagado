 
function SettingsModels() {
  const [activeModel, setActiveModel] = useState("qwen-2.5b");
  const [autoSwitch, setAutoSwitch] = useState(false);
 
  const installedModels = [
    { id: "tinyllama", name: "TinyLlama-1B-Q4", size: "300 MB", status: "cold" },
    { id: "qwen-1b", name: "Qwen3-1B-Q4", size: "500 MB", status: "cold" },
    { id: "qwen-2.5b", name: "Qwen3-2.5B-IQ4", size: "1.8 GB", status: "active" },
  ];
 
  return (
    <Card>
      <h2 className="text-h2 text-laputa-text-bright font-bold mb-6">Models</h2>
 
      <div className="space-y-6">
        <div>
          <label className="block text-body-sm text-laputa-text-dim mb-2">Active Model</label>
          <Select
            value={activeModel}
            onChange={setActiveModel}
            options={installedModels.map((m) => ({ value: m.id, label: m.name }))}
          />
        </div>
 
        <div>
          <Checkbox
            label="Auto-switch model based on task complexity"
            checked={autoSwitch}
            onChange={setAutoSwitch}
          />
        </div>
 
        <div>
          <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">Installed Models</h3>
          <div className="space-y-2">
            {installedModels.map((model) => (
              <div
                key={model.id}
                className="bg-laputa-surface-2 border border-laputa-border rounded-sm p-4 flex items-center justify-between"
              >
                <div>
                  <div className="text-body text-laputa-text-bright font-semibold">{model.name}</div>
                  <div className="flex items-center gap-3 mt-1 text-body-sm text-laputa-text-dim">
                    <span>Size: {model.size}</span>
                    <Badge variant={model.status === "active" ? "success" : "default"}>
                      {model.status === "active" ? "ACTIVE" : "COLD"}
                    </Badge>
                  </div>
                </div>
                <div className="flex gap-2">
                  <Button variant="secondary" size="sm">
                    {model.status === "active" ? "In Use" : "Activate"}
                  </Button>
                  <Button variant="secondary" size="sm">Remove</Button>
                </div>
              </div>
            ))}
          </div>
        </div>
 
        <Button variant="primary" size="md">
          + Browse Models
        </Button>
      </div>
    </Card>
  );
}
