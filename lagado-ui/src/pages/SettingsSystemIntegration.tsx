import React, { useState } from "react";
import { Card } from "../components/Card";
import { Checkbox } from "../components/Checkbox";
import { Alert } from "../components/Alert";
import { Select } from "../components/Select";
import { Button } from "../components/Button";

export default function SettingsSystemIntegration() {
  const [showTray, setShowTray] = useState(true);
  const [enableURL, setEnableURL] = useState(true);
  const [confirmActions, setConfirmActions] = useState(true);
  const [enableClipboard, setEnableClipboard] = useState(true);
  const [autoDetect, setAutoDetect] = useState(true);
  const [clearTimeout, setClearTimeout] = useState("5");

  return (
    <Card>
      <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">System Integration</h2>

      <div className="space-y-8">
        <div>
          <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">System Tray</h3>
          <div className="space-y-3">
            <Checkbox
              label="Show Lagado icon in system tray"
              checked={showTray}
              onChange={setShowTray}
            />
            <Checkbox
              label="Enable quick access menu"
              checked={true}
              onChange={() => {}}
            />
            <Checkbox
              label="Show notifications"
              checked={true}
              onChange={() => {}}
            />
          </div>
        </div>

        <div>
          <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">
            URL Handler (lagado://)
          </h3>
          <Alert
            type="warning"
            title="Security Notice"
            message="URL handlers can be triggered by external apps. We require user confirmation for all actions."
          />
          <div className="space-y-3 mt-4">
            <Checkbox
              label="Enable lagado:// URL protocol"
              checked={enableURL}
              onChange={setEnableURL}
            />
            <Checkbox
              label="Always confirm before action"
              checked={confirmActions}
              onChange={setConfirmActions}
            />
          </div>
          <div className="mt-4">
            <p className="text-body-sm text-lagado-text-dim mb-2">Allowed Actions:</p>
            <div className="space-y-2 ml-4">
              <Checkbox label="Open task" checked={true} onChange={() => {}} />
              <Checkbox label="Connect to MCP server" checked={true} onChange={() => {}} />
              <Checkbox
                label="Execute command"
                checked={false}
                onChange={() => {}}
              />
            </div>
          </div>
          <Button variant="secondary" size="sm" className="mt-3">
            Manage URL Whitelist
          </Button>
        </div>

        <div>
          <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">Clipboard</h3>
          <div className="space-y-3">
            <Checkbox
              label="Allow agent to read clipboard"
              checked={enableClipboard}
              onChange={setEnableClipboard}
            />
            <Checkbox
              label="Auto-detect formatted content"
              checked={autoDetect}
              onChange={setAutoDetect}
            />
            <Checkbox
              label="Clear sensitive data after timeout"
              checked={true}
              onChange={() => {}}
            />
          </div>
          <div className="mt-3">
            <label className="block text-body-sm text-lagado-text-dim mb-2">
              Clear Timeout
            </label>
            <Select
              value={clearTimeout}
              onChange={setClearTimeout}
              options={[
                { value: "5", label: "5 minutes" },
                { value: "15", label: "15 minutes" },
                { value: "60", label: "1 hour" },
              ]}
            />
          </div>
        </div>

        <div>
          <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">Drag & Drop</h3>
          <div className="space-y-3">
            <Checkbox
              label="Accept files dropped on Lagado"
              checked={true}
              onChange={() => {}}
            />
            <Checkbox
              label="Accept text dropped on Lagado"
              checked={true}
              onChange={() => {}}
            />
          </div>
        </div>

        <div>
          <h3 className="text-h3 text-lagado-text-bright font-semibold mb-3">File Associations</h3>
          <Checkbox
            label="Register .lagado file type"
            checked={true}
            onChange={() => {}}
          />
        </div>
      </div>
    </Card>
  );
}
