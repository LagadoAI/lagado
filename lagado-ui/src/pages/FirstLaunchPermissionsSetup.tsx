import React, { useState } from "react";
import { useNavigate } from "react-router-dom";
import { FolderOpen, AppWindow, Zap, ShieldCheck } from "lucide-react";
import { Button } from "../components/Button";
import { Checkbox } from "../components/Checkbox";
import { Card } from "../components/Card";

interface FirstLaunchPermissionsSetupProps {
  onComplete: () => void;
}

// NOTE: these are starting preferences. Enforcement is the runtime HITL gate
// (gate::evaluate_action) — Lagado confirms before any sensitive access regardless.
// Persisting these into the tool trust store is a follow-up.
export default function FirstLaunchPermissionsSetup({
  onComplete,
}: FirstLaunchPermissionsSetupProps) {
  const navigate = useNavigate();

  const [filePerms, setFilePerms] = useState({ documents: true, downloads: false, pictures: false });
  const [appPerms, setAppPerms] = useState({ firefox: true, textEditor: true, terminal: false });
  const [advanced, setAdvanced] = useState({ write: true, delete: false, execute: false });

  const finish = () => {
    onComplete();
    navigate("/chat");
  };

  const Section = ({
    icon,
    title,
    children,
  }: {
    icon: React.ReactNode;
    title: string;
    children: React.ReactNode;
  }) => (
    <Card className="mb-5">
      <h3 className="text-h3 text-lagado-text-bright font-bold mb-4 flex items-center gap-2.5">
        <span className="text-lagado-red">{icon}</span>
        {title}
      </h3>
      <div className="space-y-3">{children}</div>
    </Card>
  );

  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4 py-10">
      <div className="max-w-2xl w-full">
        <h1 className="text-h1 text-lagado-text-bright font-bold mb-2">Set Up Permissions</h1>
        <p className="text-body text-lagado-text-dim mb-8">
          Choose what Lagado can access. You can change these anytime in Settings.
        </p>

        <Section icon={<FolderOpen size={20} />} title="File System">
          <Checkbox label="Documents folder" checked={filePerms.documents} onChange={(c) => setFilePerms({ ...filePerms, documents: c })} />
          <Checkbox label="Downloads folder" checked={filePerms.downloads} onChange={(c) => setFilePerms({ ...filePerms, downloads: c })} />
          <Checkbox label="Pictures folder" checked={filePerms.pictures} onChange={(c) => setFilePerms({ ...filePerms, pictures: c })} />
        </Section>

        <Section icon={<AppWindow size={20} />} title="Applications">
          <Checkbox label="Firefox (web browser)" checked={appPerms.firefox} onChange={(c) => setAppPerms({ ...appPerms, firefox: c })} />
          <Checkbox label="Text Editor" checked={appPerms.textEditor} onChange={(c) => setAppPerms({ ...appPerms, textEditor: c })} />
          <Checkbox label="Terminal (advanced)" checked={appPerms.terminal} onChange={(c) => setAppPerms({ ...appPerms, terminal: c })} />
        </Section>

        <Section icon={<Zap size={20} />} title="Operations">
          <Checkbox label="Write to approved folders" checked={advanced.write} onChange={(c) => setAdvanced({ ...advanced, write: c })} />
          <Checkbox label="Delete files (caution)" checked={advanced.delete} onChange={(c) => setAdvanced({ ...advanced, delete: c })} />
          <Checkbox label="Execute commands (advanced)" checked={advanced.execute} onChange={(c) => setAdvanced({ ...advanced, execute: c })} />
        </Section>

        <div className="flex gap-3 mt-7">
          <Button variant="secondary" size="lg" className="px-8" onClick={finish}>
            Customize Later
          </Button>
          <Button variant="primary" size="lg" onClick={finish} className="flex-1">
            Start Using Lagado
          </Button>
        </div>

        <p className="text-caption text-lagado-text-dim text-center mt-6 flex items-center justify-center gap-1.5">
          <ShieldCheck size={14} />
          Lagado confirms with you before accessing any sensitive resource.
        </p>
      </div>
    </div>
  );
}
