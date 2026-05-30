// Modal for managing permissions
 
import React, { useState } from "react";
import { Button } from "./Button";
import { Checkbox } from "./Checkbox";
 
interface PermissionBrowserDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onApply: (permissions: any) => void;
}
 
export function PermissionBrowserDialog({
  isOpen,
  onClose,
  onApply,
}: PermissionBrowserDialogProps) {
  const [permissions, setPermissions] = useState({
    files: {
      documents: true,
      downloads: false,
      pictures: false,
      projects: true,
      music: false,
    },
    external: {
      usbDrive: true,
    },
    apps: {
      firefox: true,
      textEditor: true,
      fileManager: true,
      terminal: false,
      slack: false,
    },
    operations: {
      read: true,
      write: true,
      execute: false,
      delete: false,
    },
  });
 
  if (!isOpen) return null;
 
  const updatePermission = (category: string, key: string, value: boolean) => {
    setPermissions({
      ...permissions,
      [category]: {
        ...permissions[category as keyof typeof permissions],
        [key]: value,
      },
    });
  };
 
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center px-4">
      <div className="absolute inset-0 bg-laputa-modal-overlay" onClick={onClose} />
 
      <div className="relative bg-laputa-surface border border-laputa-border rounded-md p-6 max-w-3xl w-full max-h-[90vh] overflow-y-auto shadow-2xl">
        <h2 className="text-h2 text-laputa-text-bright font-bold mb-6">
          Select Permissions
        </h2>
 
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
          {/* File System */}
          <div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">
              File System
            </h3>
            <div className="bg-laputa-surface-2 border border-laputa-border rounded-sm p-3 space-y-2">
              <p className="text-caption text-laputa-text-dim mb-2">/home/user/</p>
              <Checkbox
                label="Documents/"
                checked={permissions.files.documents}
                onChange={(c) => updatePermission("files", "documents", c)}
              />
              <Checkbox
                label="Downloads/"
                checked={permissions.files.downloads}
                onChange={(c) => updatePermission("files", "downloads", c)}
              />
              <Checkbox
                label="Pictures/"
                checked={permissions.files.pictures}
                onChange={(c) => updatePermission("files", "pictures", c)}
              />
              <Checkbox
                label="Projects/laputa/"
                checked={permissions.files.projects}
                onChange={(c) => updatePermission("files", "projects", c)}
              />
              <Checkbox
                label="Music/"
                checked={permissions.files.music}
                onChange={(c) => updatePermission("files", "music", c)}
              />
            </div>
          </div>
 
          {/* External Storage */}
          <div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">
              External Storage
            </h3>
            <div className="bg-laputa-surface-2 border border-laputa-border rounded-sm p-3">
              <Checkbox
                label="/mnt/usb_drive/data/"
                checked={permissions.external.usbDrive}
                onChange={(c) => updatePermission("external", "usbDrive", c)}
              />
              <Button variant="secondary" size="sm" className="mt-3 w-full">
                + Add External Drive
              </Button>
            </div>
          </div>
 
          {/* Applications */}
          <div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">
              Applications
            </h3>
            <div className="bg-laputa-surface-2 border border-laputa-border rounded-sm p-3 space-y-2">
              <Checkbox
                label="Firefox (read web, form fill)"
                checked={permissions.apps.firefox}
                onChange={(c) => updatePermission("apps", "firefox", c)}
              />
              <Checkbox
                label="Text Editor (read/write)"
                checked={permissions.apps.textEditor}
                onChange={(c) => updatePermission("apps", "textEditor", c)}
              />
              <Checkbox
                label="File Manager"
                checked={permissions.apps.fileManager}
                onChange={(c) => updatePermission("apps", "fileManager", c)}
              />
              <Checkbox
                label="Terminal (advanced)"
                checked={permissions.apps.terminal}
                onChange={(c) => updatePermission("apps", "terminal", c)}
              />
              <Checkbox
                label="Slack (web control)"
                checked={permissions.apps.slack}
                onChange={(c) => updatePermission("apps", "slack", c)}
              />
            </div>
          </div>
 
          {/* Operations */}
          <div>
            <h3 className="text-h3 text-laputa-text-bright font-semibold mb-3">
              Operations
            </h3>
            <div className="bg-laputa-surface-2 border border-laputa-border rounded-sm p-3 space-y-2">
              <Checkbox
                label="Read files"
                checked={permissions.operations.read}
                onChange={(c) => updatePermission("operations", "read", c)}
              />
              <Checkbox
                label="Write to approved folders"
                checked={permissions.operations.write}
                onChange={(c) => updatePermission("operations", "write", c)}
              />
              <Checkbox
                label="Execute commands"
                checked={permissions.operations.execute}
                onChange={(c) => updatePermission("operations", "execute", c)}
              />
              <Checkbox
                label="Delete files (caution)"
                checked={permissions.operations.delete}
                onChange={(c) => updatePermission("operations", "delete", c)}
              />
            </div>
          </div>
        </div>
 
        {/* Actions */}
        <div className="flex gap-3 pt-4 border-t border-laputa-border">
          <Button variant="secondary" size="md" onClick={onClose} className="flex-1">
            Cancel
          </Button>
          <Button
            variant="primary"
            size="md"
            onClick={() => onApply(permissions)}
            className="flex-1"
          >
            Apply Selected
          </Button>
        </div>
      </div>
    </div>
  );
}
