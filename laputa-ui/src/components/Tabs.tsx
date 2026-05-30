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
    <div className={`flex border-b border-laputa-border ${className || ""}`}>
      {tabs.map((tab) => (
        <button
          key={tab.id}
          onClick={() => onTabChange(tab.id)}
          className={`
            px-4 py-3 text-body font-rajdhani transition-colors
            ${
              activeTab === tab.id
                ? "text-laputa-text-bright border-b-2 border-laputa-red"
                : "text-laputa-text-dim hover:text-laputa-text"
            }
          `}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}
 
