
import { useNavigate } from "react-router-dom";
import { Header } from "../components/Header";
import { Card } from "../components/Card";
import { Badge } from "../components/Badge";
import { MetadataList } from "../components/MetadataList";
import { Select } from "../components/Select";
import { Button } from "../components/Button";

export default function ServerManagement() {
  const navigate = useNavigate();
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title="Server & Models" />

      <div className="border-b border-lagado-border bg-lagado-surface px-4 py-3">
        <button onClick={() => navigate("/chat")} className="px-3 py-1.5 text-body-sm text-lagado-text-dim hover:text-lagado-text border border-lagado-border rounded-md hover:border-lagado-red transition-colors">
          ← Chat
        </button>
      </div>

      <div className="flex-1 p-6 max-w-3xl mx-auto w-full">
        <Card>
          <h2 className="text-h2 text-lagado-text-bright font-bold mb-6">Active Model</h2>
 
          <div className="bg-lagado-surface-2 border border-lagado-border rounded-sm p-4 mb-6">
            <div className="flex items-center justify-between mb-3">
              <span className="text-body text-lagado-text-bright font-semibold">
                Qwen3-2.5B-IQ4
              </span>
              <Badge variant="success">ACTIVE</Badge>
            </div>
            <MetadataList
              items={[
                { key: "Type", value: "Local" },
                { key: "Size", value: "2.1 GB" },
                { key: "Status", value: "Loaded" },
                { key: "Performance", value: "4.2 tokens/s" },
              ]}
            />
          </div>
 
          <div className="space-y-4">
            <div>
              <label className="block text-body-sm text-lagado-text-dim mb-2">
                Switch Model
              </label>
              <Select
                value="qwen-2.5b"
                onChange={() => {}}
                options={[
                  { value: "tinyllama", label: "TinyLlama-1B" },
                  { value: "qwen-1b", label: "Qwen3-1B" },
                  { value: "qwen-2.5b", label: "Qwen3-2.5B" },
                  { value: "qwen-8b", label: "Qwen3-8B" },
                ]}
              />
            </div>
 
            <Button variant="secondary" size="md">Configure Cloud Model</Button>
          </div>
        </Card>
 
        <Card className="mt-4">
          <h3 className="text-h3 text-lagado-text-bright font-bold mb-4">Storage</h3>
          <MetadataList
            items={[
              { key: "Cold Storage", value: "1.4 GB" },
              { key: "Hot Storage", value: "2.1 GB" },
              { key: "Available", value: "6.5 GB" },
            ]}
          />
        </Card>
      </div>
    </div>
  );
}
