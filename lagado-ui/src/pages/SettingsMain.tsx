import React, { useState } from "react";
import { Settings } from "lucide-react";
import { AppSidebar } from "../components/AppSidebar";
import { Input } from "../components/Input";
import { Select } from "../components/Select";
import { Button } from "../components/Button";
import SettingsBackup from "./SettingsBackup";
import SettingsModels from "./SettingsModels";
import SettingsEngine from "./SettingsEngine";
import SettingsInference from "./SettingsInference";
import SettingsKVCache from "./SettingsKVCache";
import SettingsPermissions from "./SettingsPermissions";
import SettingsVault from "./SettingsVault";
import SettingsSystemIntegration from "./SettingsSystemIntegration";
import SettingsNetwork from "./SettingsNetwork";
import SettingsAppConnections from "./SettingsAppConnections";
import SettingsAdvanced from "./SettingsAdvanced";

const TABS = [
  { id: "profile",     label: "Profile" },
  { id: "models",      label: "Models" },
  { id: "engine",      label: "Engine" },
  { id: "inference",   label: "Inference" },
  { id: "kv-cache",    label: "KV Cache" },
  { id: "permissions", label: "Permissions" },
  { id: "vault",       label: "Vault" },
  { id: "backup",      label: "Backup" },
  { id: "system",      label: "System" },
  { id: "network",     label: "Network" },
  { id: "apps",        label: "Apps" },
  { id: "advanced",    label: "Advanced" },
];

export default function SettingsMain() {
  const [activeTab, setActiveTab] = useState("models");
  const [username, setUsername] = useState(localStorage.getItem('lagado_username') || 'local_user');
  const [theme, setTheme] = useState("dark");
  const [timeout, setTimeout] = useState("60");

  const saveProfile = () => {
    localStorage.setItem('lagado_username', username);
  };

  return (
    <div style={{ height: "100vh", background: "var(--bg)", display: "flex", overflow: "hidden" }}>
      <AppSidebar />

      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
        {/* Topbar */}
        <div style={{
          height: 52, flexShrink: 0, display: "flex", alignItems: "center", gap: 12, padding: "0 16px",
          borderBottom: "1px solid var(--line-700)",
          background: "var(--glass-opaque)",
        }}>
          <Settings size={18} style={{ color: "var(--text-dim)" }} />
          <span style={{ fontFamily: "var(--font-display)", fontWeight: 600, fontSize: 16, color: "var(--text-strong)" }}>Settings</span>
        </div>

        {/* Content */}
        <div style={{ flex: 1, overflowY: "auto" }}>
          <div style={{ maxWidth: 760, margin: "0 auto", padding: "20px 24px" }}>
            {/* Tab bar */}
            <div className="lg-tabs">
              {TABS.map(t => (
                <button
                  key={t.id}
                  className={`lg-tab ${activeTab === t.id ? "lg-tab--active" : ""}`}
                  onClick={() => setActiveTab(t.id)}
                >
                  {t.label}
                </button>
              ))}
            </div>

            {/* Tab panels */}
            <div style={{ marginTop: 20, display: "flex", flexDirection: "column", gap: 16 }}>
              {activeTab === "profile" && (
                <div className="lg-card">
                  <div className="lg-card__header">
                    <h3 style={{ fontSize: 16, color: "var(--text-strong)", fontFamily: "var(--font-display)", fontWeight: 600 }}>Profile</h3>
                  </div>
                  <div className="lg-card__body" style={{ display: "flex", flexDirection: "column", gap: 14, maxWidth: 360 }}>
                    <div>
                      <label className="lg-label">Username</label>
                      <Input value={username} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setUsername(e.target.value)} />
                    </div>
                    <div>
                      <label className="lg-label">Theme</label>
                      <Select value={theme} onChange={setTheme} options={[
                        { value: "dark", label: "Dark" },
                        { value: "auto", label: "Auto" },
                      ]} />
                    </div>
                    <div>
                      <label className="lg-label">Session timeout</label>
                      <Select value={timeout} onChange={setTimeout} options={[
                        { value: "60", label: "1 hour" },
                        { value: "480", label: "8 hours" },
                        { value: "never", label: "Never" },
                      ]} />
                    </div>
                    <Button variant="primary" size="md" onClick={saveProfile} style={{ alignSelf: "flex-start" }}>
                      Save changes
                    </Button>
                  </div>
                </div>
              )}
              {activeTab === "models"      && <SettingsModels />}
              {activeTab === "engine"      && <SettingsEngine />}
              {activeTab === "inference"   && <SettingsInference />}
              {activeTab === "kv-cache"    && <SettingsKVCache />}
              {activeTab === "permissions" && <SettingsPermissions />}
              {activeTab === "vault"       && <SettingsVault />}
              {activeTab === "backup"      && <SettingsBackup />}
              {activeTab === "system"      && <SettingsSystemIntegration />}
              {activeTab === "network"     && <SettingsNetwork />}
              {activeTab === "apps"        && <SettingsAppConnections />}
              {activeTab === "advanced"    && <SettingsAdvanced />}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
