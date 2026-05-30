 
function SettingsBackup() {
  const [backupMethod, setBackupMethod] = useState("local");
  const [frequency, setFrequency] = useState("weekly");
  const [autoBackup, setAutoBackup] = useState(true);
 
  return (
    <Card>
      <h2 className="text-h2 text-laputa-text-bright font-bold mb-6">Backup & Sync</h2>
 
      <div className="space-y-6">
        <div>
          <label className="block text-body-sm text-laputa-text-dim mb-3">Backup Method</label>
          <div className="space-y-2">
            <Radio
              name="backup"
              value="local"
              checked={backupMethod === "local"}
              onChange={() => setBackupMethod("local")}
              label="Local only"
              description="Backup to local storage, never sent to cloud"
            />
            <Radio
              name="backup"
              value="cloud"
              checked={backupMethod === "cloud"}
              onChange={() => setBackupMethod("cloud")}
              label="Auto cloud (encrypted)"
              description="End-to-end encrypted backup to your cloud"
            />
            <Radio
              name="backup"
              value="manual"
              checked={backupMethod === "manual"}
              onChange={() => setBackupMethod("manual")}
              label="Manual only"
              description="Backup when you decide"
            />
            <Radio
              name="backup"
              value="none"
              checked={backupMethod === "none"}
              onChange={() => setBackupMethod("none")}
              label="No backup"
              description="Risk: data loss"
            />
          </div>
        </div>
 
        {backupMethod === "cloud" && (
          <>
            <div>
              <label className="block text-body-sm text-laputa-text-dim mb-2">Cloud Provider</label>
              <Select
                value="none"
                onChange={() => {}}
                options={[
                  { value: "none", label: "Select provider..." },
                  { value: "icloud", label: "iCloud" },
                  { value: "gdrive", label: "Google Drive" },
                  { value: "dropbox", label: "Dropbox" },
                  { value: "custom", label: "Custom S3" },
                ]}
              />
            </div>
            <div>
              <label className="block text-body-sm text-laputa-text-dim mb-2">Frequency</label>
              <Select
                value={frequency}
                onChange={setFrequency}
                options={[
                  { value: "daily", label: "Daily" },
                  { value: "weekly", label: "Weekly" },
                  { value: "monthly", label: "Monthly" },
                ]}
              />
            </div>
          </>
        )}
 
        <div className="pt-4 border-t border-laputa-border">
          <p className="text-body-sm text-laputa-text-dim mb-3">
            Last backup: 5/26/2026 14:30
          </p>
          <Button variant="primary" size="md">
            Backup Now
          </Button>
        </div>
      </div>
    </Card>
  );
}
