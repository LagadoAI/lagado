 
import React, { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "../components/Button";
import { Checkbox } from "../components/Checkbox";
import { Card } from "../components/Card";
 
interface FirstLaunchPermissionsSetupProps {
  onComplete: () => void;
}
 
export default function FirstLaunchPermissionsSetup({
  onComplete,
}: FirstLaunchPermissionsSetupProps) {
  const navigate = useNavigate();
 
  const [filePerms, setFilePerms] = useState({
    documents: true,
    downloads: false,
    pictures: false,
  });
 
  const [appPerms, setAppPerms] = useState({
    firefox: true,
    textEditor: true,
    terminal: false,
  });
 
  const [advanced, setAdvanced] = useState({
    write: true,
    delete: false,
    execute: false,
  });
 
  const handleComplete = () => {
    onComplete();
    navigate("/chat");
  };
 
  return (
    <div className="min-h-screen bg-lagado-bg flex items-center justify-center px-4">
      <div className="max-w-2xl w-full py-8">
        <h1 className="text-h1 text-lagado-text-bright font-bold mb-2">
          Set Up Permissions
        </h1>
        <p className="text-body text-lagado-text-dim mb-8">
          Choose what Lagado can access. You can change these anytime in Settings.
        </p>
 
        {/* File System */}
        <Card className="mb-6">
          <h3 className="text-h3 text-lagado-text-bright font-bold mb-4 flex items-center gap-2">
            <span className="text-2xl">📁</span> File System
          </h3>
          <div className="space-y-3">
            <Checkbox
              label="Documents folder"
              checked={filePerms.documents}
              onChange={(c) => setFilePerms({ ...filePerms, documents: c })}
            />
            <Checkbox
              label="Downloads folder"
              checked={filePerms.downloads}
              onChange={(c) => setFilePerms({ ...filePerms, downloads: c })}
            />
            <Checkbox
              label="Pictures folder"
              checked={filePerms.pictures}
              onChange={(c) => setFilePerms({ ...filePerms, pictures: c })}
            />
          </div>
        </Card>
 
        {/* Applications */}
        <Card className="mb-6">
          <h3 className="text-h3 text-lagado-text-bright font-bold mb-4 flex items-center gap-2">
            <span className="text-2xl">📱</span> Applications
          </h3>
          <div className="space-y-3">
            <Checkbox
              label="Firefox (web browser)"
              checked={appPerms.firefox}
              onChange={(c) => setAppPerms({ ...appPerms, firefox: c })}
            />
            <Checkbox
              label="Text Editor"
              checked={appPerms.textEditor}
              onChange={(c) => setAppPerms({ ...appPerms, textEditor: c })}
            />
            <Checkbox
              label="Terminal (advanced)"
              checked={appPerms.terminal}
              onChange={(c) => setAppPerms({ ...appPerms, terminal: c })}
            />
          </div>
        </Card>
 
        {/* Advanced */}
        <Card className="mb-6">
          <h3 className="text-h3 text-lagado-text-bright font-bold mb-4 flex items-center gap-2">
            <span className="text-2xl">⚡</span> Operations
          </h3>
          <div className="space-y-3">
            <Checkbox
              label="Write to approved folders"
              checked={advanced.write}
              onChange={(c) => setAdvanced({ ...advanced, write: c })}
            />
            <Checkbox
              label="Delete files (caution)"
              checked={advanced.delete}
              onChange={(c) => setAdvanced({ ...advanced, delete: c })}
            />
            <Checkbox
              label="Execute commands (advanced)"
              checked={advanced.execute}
              onChange={(c) => setAdvanced({ ...advanced, execute: c })}
            />
          </div>
        </Card>
 
        <div className="flex gap-3">
          <Button variant="primary" size="lg" onClick={handleComplete} className="flex-1">
            Start Using Lagado
          </Button>
          <Button variant="secondary" size="lg" className="flex-1">
            Customize Later
          </Button>
        </div>
 
        <p className="text-caption text-lagado-text-dim text-center mt-6">
          You'll be prompted before Lagado accesses any approved resource
        </p>
      </div>
    </div>
  );
}
 

