// Tab navigation component
 
interface Tab {
  id: string;
  label: string;
}
 
interface TabsProps {
  tabs: Tab[];
  activeTab: string;
  onTabChange: (tabId: string) => void;
  className?: string;
}
 
export function Tabs({ tabs, activeTab, onTabChange, className }: TabsProps) {
  return (
    <div className={`flex border-b border-lagado-border ${className || ""}`}>
      {tabs.map((tab) => (
        <button
          key={tab.id}
          onClick={() => onTabChange(tab.id)}
          className={`
            px-4 py-3 text-body font-rajdhani transition-colors
            ${
              activeTab === tab.id
                ? "text-lagado-text-bright border-b-2 border-lagado-red"
                : "text-lagado-text-dim hover:text-lagado-text"
            }
          `}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}
 
