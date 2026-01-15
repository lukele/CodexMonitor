import { useMemo } from "react";
import { FileDiff } from "lucide-react";

type TurnDiffPanelProps = {
  diff: string | null;
  isProcessing: boolean;
};

type SimpleDiffLine = {
  type: "add" | "del" | "context" | "header" | "hunk";
  text: string;
};

function parseSimpleDiff(diff: string): SimpleDiffLine[] {
  const lines = diff.split("\n");
  const parsed: SimpleDiffLine[] = [];
  
  for (const line of lines) {
    if (line.startsWith("--- ") || line.startsWith("+++ ")) {
      parsed.push({ type: "header", text: line });
    } else if (line.startsWith("@@")) {
      parsed.push({ type: "hunk", text: line });
    } else if (line.startsWith("+")) {
      parsed.push({ type: "add", text: line.slice(1) });
    } else if (line.startsWith("-")) {
      parsed.push({ type: "del", text: line.slice(1) });
    } else if (line.startsWith(" ")) {
      parsed.push({ type: "context", text: line.slice(1) });
    } else if (line.trim()) {
      // Treat non-empty lines without prefix as context
      parsed.push({ type: "context", text: line });
    }
  }
  
  return parsed;
}

export function TurnDiffPanel({ diff, isProcessing }: TurnDiffPanelProps) {
  const hasDiff = diff && diff.trim().length > 0;
  const emptyLabel = isProcessing ? "Waiting for changes..." : "No changes in this turn.";
  
  const parsedLines = useMemo(() => {
    if (!diff) return [];
    return parseSimpleDiff(diff);
  }, [diff]);

  return (
    <aside className="turn-diff-panel">
      <div className="turn-diff-header">
        <FileDiff className="turn-diff-icon" size={14} />
        <span>Turn Changes</span>
      </div>
      {hasDiff ? (
        <div className="turn-diff-content">
          {parsedLines.map((line, i) => (
            <div key={i} className={`turn-diff-line turn-diff-line-${line.type}`}>
              {line.type === "add" && <span className="turn-diff-prefix">+</span>}
              {line.type === "del" && <span className="turn-diff-prefix">-</span>}
              {line.type === "context" && <span className="turn-diff-prefix"> </span>}
              <span className="turn-diff-text">{line.text || "\u00A0"}</span>
            </div>
          ))}
        </div>
      ) : (
        <div className="turn-diff-empty">{emptyLabel}</div>
      )}
    </aside>
  );
}
