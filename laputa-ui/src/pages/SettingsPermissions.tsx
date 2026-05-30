 
function SettingsPermissions() {
  const [autoApprove, setAutoApprove] = useState("restore");
 
  return (
    <Card>
      <h2 className="text-h2 text-laputa-text-bright font-bold mb-6">Permissions</h2>
 
      <div className="space-y-6">
        <div>
          <label className="block text-body-sm text-laputa-text-dim mb-3">
            Auto-approval Mode
          </label>
          <div className="space-y-2">
            <Radio
              name="approval"
              value="restore"
              checked={autoApprove === "restore"}
              onChange={() => setAutoApprove("restore")}
              label="Restore last session"
              description="Use the same approvals as last time"
            />
            <Radio
              name="approval"
              value="fresh"
              checked={autoApprove === "fresh"}
              onChange={() => setAutoApprove("fresh")}
              label="Start fresh each session"
              description="Always prompt for permissions on launch"
            />
          </div>
        </div>
 
        <div>
          <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">Current Approvals</h3>
          <div className="space-y-2">
            <div className="flex items-center justify-between p-3 bg-laputa-surface-2 border border-laputa-border rounded-sm">
              <span className="text-body text-laputa-text">/home/user/Documents/</span>
              <button className="text-laputa-red hover:text-laputa-text-bright">Revoke</button>
            </div>
            <div className="flex items-center justify-between p-3 bg-laputa-surface-2 border border-laputa-border rounded-sm">
              <span className="text-body text-laputa-text">Firefox</span>
              <button className="text-laputa-red hover:text-laputa-text-bright">Revoke</button>
            </div>
          </div>
        </div>
 
        <div className="pt-4 border-t border-laputa-border">
          <Button variant="primary" size="md">Manage Permissions</Button>
          <Button variant="secondary" size="md" className="ml-3">
            Clear All Approvals
          </Button>
        </div>
      </div>
    </Card>
  );
}
