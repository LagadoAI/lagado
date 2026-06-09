// Main settings hub with navigation tabs

import React, { useState } from "react";
import { useNavigate } from "react-router-dom";
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
  const navigate = useNavigate();
  const [activeTab, setActiveTab] = useState("profile");
  const [username, setUsername] = useState("lagado_user");
  const [theme, setTheme] = useState("dark");
  const [timeout, setTimeout] = useState("60");
 
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Settings" />

      <div className="border-b border-lagado-border bg-lagado-surface px-4 py-3">
        <button onClick={() => navigate("/chat")} className="px-3 py-1.5 text-body-sm text-lagado-text-dim hover:text-lagado-text border border-lagado-border rounded-md hover:border-lagado-red transition-colors">
          ← Chat
        </button>
      </div>

      <div className="flex-1 max-w-5xl mx-auto w-full">
        {/* Tabs */}
        <div className="px-6 pt-4">
          <Tabs tabs={settingsTabs} activeTab={activeTab} onTabChange={setActiveTab} />
        </div>
 
        {/* Content */}
        <div className="p-6">
          {activeTab === "profile" && (
            <Card>
              <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Profile</h2>
              <div className="space-y-6">
                <div>
                  <label className="block text-body-sm text-lagado-text-dim mb-2">Username</label>
                  <Input value={username} onChange={(e) => setUsername(e.target.value)} />
                </div>
 
                <div>
                  <label className="block text-body-sm text-lagado-text-dim mb-2">Avatar</label>
                  <div className="w-20 h-20 bg-lagado-surface-2 rounded-lg flex items-center justify-center border border-lagado-border">
                    <span className="text-4xl">⚔</span>
                  </div>
                </div>
 
                <div>
                  <label className="block text-body-sm text-lagado-text-dim mb-2">Theme</label>
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
                  <label className="block text-body-sm text-lagado-text-dim mb-2">Session Timeout</label>
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
