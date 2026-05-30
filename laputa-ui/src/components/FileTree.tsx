// FILE: src/components/FileTree.tsx
// File browser tree component
 
interface FileNode {
  id: string;
  name: string;
  type: "file" | "folder";
  children?: FileNode[];
}
 
interface FileTreeProps {
  files: FileNode[];
  onSelect?: (file: FileNode) => void;
  className?: string;
}
 
export function FileTree({ files, onSelect, className }: FileTreeProps) {
  const [expanded, setExpanded] = React.useState<Set<string>>(
    new Set()
  );
 
  const toggleExpanded = (id: string) => {
    const newExpanded = new Set(expanded);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpanded(newExpanded);
  };
 
  const renderNode = (node: FileNode, level: number = 0) => (
    <div key={node.id}>
      <div
        className={`
          flex items-center gap-2 px-2 py-1 cursor-pointer
          hover:bg-laputa-surface-2 rounded transition-colors
          ${level > 0 ? `ml-${level * 4}` : ""}
        `}
        onClick={() => {
          if (node.type === "folder") {
            toggleExpanded(node.id);
          } else {
            onSelect?.(node);
          }
        }}
      >
        {node.type === "folder" && (
          <span className="text-laputa-text-dim">
            {expanded.has(node.id) ? "▼" : "▶"}
          </span>
        )}
        <span className="text-laputa-text">
          {node.type === "folder" ? "📁" : "📄"}
        </span>
        <span className="text-body-sm text-laputa-text">{node.name}</span>
      </div>
      {node.type === "folder" &&
        expanded.has(node.id) &&
        node.children?.map((child) => renderNode(child, level + 1))}
    </div>
  );
 
  return (
    <div className={`font-mono text-sm ${className || ""}`}>
      {files.map((file) => renderNode(file))}
    </div>
  );
}

