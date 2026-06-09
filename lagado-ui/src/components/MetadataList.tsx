// Display key-value pairs
 
interface MetadataItem {
  key: string;
  value: string | React.ReactNode;
  mono?: boolean;
}
 
interface MetadataListProps {
  items: MetadataItem[];
  className?: string;
}
 
export function MetadataList({ items, className }: MetadataListProps) {
  return (
    <div className={`space-y-2 ${className || ""}`}>
      {items.map((item, idx) => (
        <div key={idx} className="flex justify-between">
          <span className="text-body-sm text-lagado-text-dim">{item.key}:</span>
          <span
            className={`text-body-sm text-lagado-text-bright ${
              item.mono ? "font-mono" : ""
            }`}
          >
            {item.value}
          </span>
        </div>
      ))}
    </div>
  );
}
