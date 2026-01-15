import { useMemo } from "react";
import { highlightLine, languageFromPath } from "../utils/syntax";

type CodeBlockProps = {
  code: string;
  language?: string | null;
  filePath?: string | null;
  showLineNumbers?: boolean;
  className?: string;
};

export function CodeBlock({
  code,
  language,
  filePath,
  showLineNumbers = true,
  className = "",
}: CodeBlockProps) {
  const detectedLanguage = language ?? languageFromPath(filePath);
  
  const highlightedLines = useMemo(() => {
    const lines = code.split("\n");
    return lines.map((line) => highlightLine(line, detectedLanguage));
  }, [code, detectedLanguage]);

  return (
    <div className={`code-block ${className}`}>
      <pre>
        <code>
          {highlightedLines.map((html, i) => (
            <div key={i} className="code-block-line">
              {showLineNumbers && (
                <span className="code-block-line-number">{i + 1}</span>
              )}
              <span
                className="code-block-line-content"
                dangerouslySetInnerHTML={{ __html: html || "\u00A0" }}
              />
            </div>
          ))}
        </code>
      </pre>
    </div>
  );
}
