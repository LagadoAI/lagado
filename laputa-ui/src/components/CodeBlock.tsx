// Syntax highlighted code block
 
interface CodeBlockProps {
  code: string;
  language?: string;
  showLineNumbers?: boolean;
  className?: string;
}
 
export function CodeBlock({
  code,
  language = "javascript",
  showLineNumbers = true,
  className,
}: CodeBlockProps) {
  const lines = code.split("\n");
 
  return (
    <pre
      className={`
        bg-laputa-bg border border-laputa-border rounded-sm p-4
        overflow-x-auto font-mono text-sm
        ${className || ""}
      `}
    >
      <code className="text-laputa-text">
        {lines.map((line, idx) => (
          <div key={idx} className="flex">
            {showLineNumbers && (
              <span className="text-laputa-text-dim mr-4 w-8 text-right select-none">
                {idx + 1}
              </span>
            )}
            <span>{line}</span>
          </div>
        ))}
      </code>
    </pre>
  );
}
