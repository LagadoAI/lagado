 
function SettingsKVCache() {
  const [cacheLocation, setCacheLocation] = useState("auto");
  const [cacheSize, setCacheSize] = useState("auto");
 
  return (
    <Card>
      <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">KV Cache Configuration</h2>
 
      <div className="space-y-6">
        <div>
          <label className="block text-body-sm text-lagado-text-dim mb-3">Cache Location</label>
          <div className="space-y-2">
            <Radio
              name="kvcache"
              value="auto"
              checked={cacheLocation === "auto"}
              onChange={() => setCacheLocation("auto")}
              label="Auto (Recommended)"
              description="Detect best location based on hardware"
            />
            <Radio
              name="kvcache"
              value="vram"
              checked={cacheLocation === "vram"}
              onChange={() => setCacheLocation("vram")}
              label="VRAM only"
              description="GPU memory only - fastest"
            />
            <Radio
              name="kvcache"
              value="ram"
              checked={cacheLocation === "ram"}
              onChange={() => setCacheLocation("ram")}
              label="RAM only"
              description="System RAM only - slower, but no GPU needed"
            />
            <Radio
              name="kvcache"
              value="split"
              checked={cacheLocation === "split"}
              onChange={() => setCacheLocation("split")}
              label="Split VRAM + RAM"
              description="Best of both - ⚠ Configurable"
            />
            <Radio
              name="kvcache"
              value="off-gpu"
              checked={cacheLocation === "off-gpu"}
              onChange={() => setCacheLocation("off-gpu")}
              label="Off-GPU for MoE"
              description="⚠ Advanced: For Mixture-of-Experts models"
            />
          </div>
        </div>
 
        <Alert
          type="warning"
          title="Performance Impact"
          message="Changing KV cache location may impact performance. Test before applying."
        />
 
        <div>
          <label className="block text-body-sm text-lagado-text-dim mb-2">Cache Size</label>
          <Select
            value={cacheSize}
            onChange={setCacheSize}
            options={[
              { value: "auto", label: "Auto" },
              { value: "1gb", label: "1 GB" },
              { value: "2gb", label: "2 GB" },
              { value: "4gb", label: "4 GB" },
              { value: "8gb", label: "8 GB" },
            ]}
          />
        </div>
 
        <Button variant="primary" size="md">Apply Settings</Button>
      </div>
    </Card>
  );
}
