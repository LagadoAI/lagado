import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Card } from "../components/Card";
import { Checkbox } from "../components/Checkbox";
import { Radio } from "../components/Radio";
import { Input } from "../components/Input";
import { Button } from "../components/Button";
import { Alert } from "../components/Alert";

interface NetworkSettings {
  proxy_enabled: boolean;
  proxy_type: string;
  proxy_host: string;
  proxy_port: number;
  bridge_address: string;
}

const DEFAULT: NetworkSettings = {
  proxy_enabled: false,
  proxy_type: "socks5",
  proxy_host: "127.0.0.1",
  proxy_port: 9050,
  bridge_address: "",
};

export default function SettingsNetwork() {
  const [settings, setSettings] = useState<NetworkSettings>(DEFAULT);
  const [saved, setSaved] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);

  useEffect(() => {
    invoke<NetworkSettings>("get_network_settings")
      .then(setSettings)
      .catch(() => {});
  }, []);

  const update = (patch: Partial<NetworkSettings>) =>
    setSettings((s) => ({ ...s, ...patch }));

  const save = () => {
    invoke("save_network_settings", { settings })
      .then(() => {
        setSaved(true);
        setTimeout(() => setSaved(false), 2000);
      })
      .catch(console.error);
  };

  const testConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const result = await invoke<string>("test_network_connection");
      setTestResult(result);
    } catch {
      setTestResult("Connection test failed — check proxy address and port.");
    } finally {
      setTesting(false);
    }
  };

  const applyPreset = (host: string, port: number) => {
    update({ proxy_enabled: true, proxy_type: "socks5", proxy_host: host, proxy_port: port });
  };

  return (
    <Card>
      <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Network & Privacy</h2>

      <div className="space-y-8">
        {/* Proxy toggle */}
        <div>
          <Checkbox
            label="Route all agent network traffic through a proxy"
            checked={settings.proxy_enabled}
            onChange={(v) => update({ proxy_enabled: v })}
          />
          <p className="text-caption text-lagado-text-dim mt-1 ml-6">
            Disabled by default. Enable to route web search, fetch, and download through Tor or any SOCKS5/HTTP proxy.
          </p>
        </div>

        {settings.proxy_enabled && (
          <>
            {/* Quick presets */}
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-3">Quick Presets</label>
              <div className="flex gap-2">
                <button
                  onClick={() => applyPreset("127.0.0.1", 9050)}
                  className="px-3 py-1.5 text-body-sm border border-lagado-border rounded-md text-lagado-text hover:border-lagado-purple hover:text-lagado-purple transition-colors"
                >
                  Tor (localhost)
                </button>
                <button
                  onClick={() => applyPreset("10.152.152.10", 9050)}
                  className="px-3 py-1.5 text-body-sm border border-lagado-border rounded-md text-lagado-text hover:border-lagado-purple hover:text-lagado-purple transition-colors"
                >
                  Whonix Gateway
                </button>
              </div>
            </div>

            {/* Proxy type */}
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-3">Proxy Type</label>
              <div className="space-y-2">
                <Radio
                  name="proxy_type"
                  value="socks5"
                  checked={settings.proxy_type === "socks5"}
                  onChange={() => update({ proxy_type: "socks5" })}
                  label="SOCKS5"
                  description="Required for Tor and Whonix. Recommended."
                />
                <Radio
                  name="proxy_type"
                  value="http"
                  checked={settings.proxy_type === "http"}
                  onChange={() => update({ proxy_type: "http" })}
                  label="HTTP / HTTPS"
                  description="For HTTP proxy servers."
                />
              </div>
            </div>

            {/* Host + port */}
            <div className="grid grid-cols-3 gap-3">
              <div className="col-span-2">
                <label className="block text-body-sm text-lagado-text-dim mb-2">Proxy Host</label>
                <Input
                  value={settings.proxy_host}
                  onChange={(e) => update({ proxy_host: e.target.value })}
                  placeholder="127.0.0.1"
                />
              </div>
              <div>
                <label className="block text-body-sm text-lagado-text-dim mb-2">Port</label>
                <Input
                  type="number"
                  value={String(settings.proxy_port)}
                  onChange={(e) => update({ proxy_port: parseInt(e.target.value, 10) || 9050 })}
                  placeholder="9050"
                />
              </div>
            </div>

            {/* Bridge address */}
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                Bridge Address <span className="text-lagado-text-dim font-normal">(optional)</span>
              </label>
              <Input
                value={settings.bridge_address}
                onChange={(e) => update({ bridge_address: e.target.value })}
                placeholder="obfs4 192.0.2.1:443 ..."
              />
              <p className="text-caption text-lagado-text-dim mt-1">
                Tor bridge for censored networks. Leave blank if not needed.
              </p>
            </div>

            {/* Test */}
            <div className="flex items-center gap-3">
              <Button variant="secondary" size="sm" onClick={testConnection}>
                {testing ? "Testing…" : "Test Connection"}
              </Button>
              {testResult && (
                <span className={`text-body-sm ${testResult.includes("failed") || testResult.includes("error") ? "text-lagado-red" : "text-lagado-green"}`}>
                  {testResult}
                </span>
              )}
            </div>

            <Alert
              type="info"
              title="Proxy scope"
              message="Only outbound agent network tools (web_search, fetch_url, read_webpage, download_file) use this proxy. Llama-server inference traffic stays on localhost and is never proxied."
            />
          </>
        )}

        {/* Save */}
        <div className="pt-4 border-t border-lagado-border flex items-center gap-3">
          <Button variant="primary" size="md" onClick={save}>
            Save
          </Button>
          {saved && (
            <span className="text-body-sm text-lagado-green">Saved</span>
          )}
        </div>
      </div>
    </Card>
  );
}
