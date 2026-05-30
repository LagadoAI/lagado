 
function SettingsVault() {
  const [maxSize, setMaxSize] = useState(10);
  const [pruningPolicy, setPruningPolicy] = useState("lru");
 
  return (
    <Card>
      <h2 className="text-h2 text-laputa-text-bright font-bold mb-6">Vault Configuration</h2>
 
      <div className="space-y-6">
        <div>
          <Slider
            min={1}
            max={50}
            value={maxSize}
            onChange={setMaxSize}
            label={`Max Vault Size (${maxSize} GB)`}
          />
        </div>
 
        <div>
          <label className="block text-body-sm text-laputa-text-dim mb-2">Pruning Policy</label>
          <Select
            value={pruningPolicy}
            onChange={setPruningPolicy}
            options={[
              { value: "lru", label: "Least Recently Used (LRU)" },
              { value: "manual", label: "Manual only" },
              { value: "off", label: "No pruning" },
            ]}
          />
        </div>
 
        <div>
          <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">External Connections</h3>
          <div className="space-y-2">
            <Button variant="secondary" size="md" className="w-full">+ Add Obsidian vault</Button>
            <Button variant="secondary" size="md" className="w-full">+ Add Database</Button>
            <Button variant="secondary" size="md" className="w-full">+ Add Cloud Drive</Button>
          </div>
        </div>
 
        <div className="pt-4 border-t border-laputa-border">
          <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">Encryption</h3>
          <Button variant="secondary" size="md">Change Encryption Key</Button>
        </div>
      </div>
    </Card>
  );
}
