// Main settings hub with navigation tabs
 
import React, { useState } from "react";
import { Header } from "../components/Header";
import { Card } from "../components/Card";
import { Input } from "../components/Input";
import { Select } from "../components/Select";
import { Button } from "../components/Button";
import { Tabs } from "../components/Tabs";
import { Checkbox } from "../components/Checkbox";
 
const settingsTabs = [
  { id: "profile", label: "Profile" },
  { id: "backup", label: "Backup" },
  { id: "models", label: "Models" },
  { id: "inference", label: "Inference" },
  { id: "kv-cache", label: "KV Cache" },
  { id: "permissions", label: "Permissions" },
  { id: "vault", label: "Vault" },
  { id: "system", label: "System" },
  { id: "apps", label: "Apps" },
  { id: "advanced", label: "Advanced" },
];
 
export default function SettingsMain() {
  const [activeTab, setActiveTab] = useState("profile");
  const [username, setUsername] = useState("laputa_user");
  const [theme, setTheme] = useState("dark");
  const [timeout, setTimeout] = useState("60");
 
  return (
    <div className="min-h-screen bg-laputa-bg flex flex-col">
      <Header title="Settings" />
 
      <div className="flex-1 max-w-5xl mx-auto w-full">
        {/* Tabs */}
        <div className="px-6 pt-4">
          <Tabs tabs={settingsTabs} activeTab={activeTab} onTabChange={setActiveTab} />
        </div>
 
        {/* Content */}
        <div className="p-6">
          {activeTab === "profile" && (
            <Card>
              <h2 className="text-h2 text-laputa-text-bright font-bold mb-6">Profile</h2>
              <div className="space-y-6">
                <div>
                  <label className="block text-body-sm text-laputa-text-dim mb-2">Username</label>
                  <Input value={username} onChange={(e) => setUsername(e.target.value)} />
                </div>
 
                <div>
                  <label className="block text-body-sm text-laputa-text-dim mb-2">Avatar</label>
                  <div className="w-20 h-20 bg-laputa-surface-2 rounded-lg flex items-center justify-center border border-laputa-border">
                    <span className="text-4xl">⚔</span>
                  </div>
                </div>
 
                <div>
                  <label className="block text-body-sm text-laputa-text-dim mb-2">Theme</label>
                  <Select
                    value={theme}
                    onChange={setTheme}
                    options={[
                      { value: "dark", label: "Dark" },
                      { value: "light", label: "Light" },
                      { value: "auto", label: "Auto" },
                    ]}
                  />
                </div>
 
                <div>
                  <label className="block text-body-sm text-laputa-text-dim mb-2">Session Timeout</label>
                  <Select
                    value={timeout}
                    onChange={setTimeout}
                    options={[
                      { value: "30", label: "30 minutes" },
                      { value: "60", label: "1 hour" },
                      { value: "480", label: "8 hours" },
                      { value: "never", label: "Never" },
                    ]}
                  />
                </div>
 
                <Button variant="primary" size="md">Save Changes</Button>
              </div>
            </Card>
          )}
 
          {activeTab === "backup" && <SettingsBackup />}
          {activeTab === "models" && <SettingsModels />}
          {activeTab === "inference" && <SettingsInference />}
          {activeTab === "kv-cache" && <SettingsKVCache />}
          {activeTab === "permissions" && <SettingsPermissions />}
          {activeTab === "vault" && <SettingsVault />}
          {activeTab === "system" && <SettingsSystemIntegration />}
          {activeTab === "apps" && <SettingsAppConnections />}
          {activeTab === "advanced" && <SettingsAdvanced />}
        </div>
      </div>
    </div>
  );
}
