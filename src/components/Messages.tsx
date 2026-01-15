import { memo, useEffect, useRef, useState } from "react";
import type { ConversationItem } from "../types";
import { Markdown } from "./Markdown";
import { DiffBlock } from "./DiffBlock";
import { CodeBlock } from "./CodeBlock";
import { languageFromPath } from "../utils/syntax";

export type ToolPreview = {
  type: 'read' | 'edit' | 'write' | 'command';
  path?: string;
  content?: string;
  diff?: string;
  language?: string;
};

type MessagesProps = {
  items: ConversationItem[];
  isThinking: boolean;
  processingStartedAt?: number | null;
  lastDurationMs?: number | null;
  onToolPreview?: (preview: ToolPreview | null) => void;
};

type ToolSummary = {
  label: string;
  value?: string;
  detail?: string;
  output?: string;
};

type StatusTone = "completed" | "processing" | "failed" | "unknown";

function basename(path: string) {
  if (!path) {
    return "";
  }
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}

function parseToolArgs(detail: string) {
  if (!detail) {
    return null;
  }
  try {
    return JSON.parse(detail) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function firstStringField(
  source: Record<string, unknown> | null,
  keys: string[],
) {
  if (!source) {
    return "";
  }
  for (const key of keys) {
    const value = source[key];
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return "";
}

function toolNameFromTitle(title: string) {
  if (!title.toLowerCase().startsWith("tool:")) {
    return "";
  }
  const [, toolPart = ""] = title.split(":");
  const segments = toolPart.split("/").map((segment) => segment.trim());
  return segments.length ? segments[segments.length - 1] : "";
}

function buildToolSummary(
  item: Extract<ConversationItem, { kind: "tool" }>,
  commandText: string,
): ToolSummary {
  if (item.toolType === "commandExecution") {
    const cleanedCommand = cleanCommandText(commandText);
    return {
      label: "command",
      value: cleanedCommand || "Command",
      detail: "",
      output: item.output || "",
    };
  }

  if (item.toolType === "webSearch") {
    return {
      label: "searched",
      value: item.detail || "",
    };
  }

  if (item.toolType === "imageView") {
    const file = basename(item.detail || "");
    return {
      label: "read",
      value: file || "image",
    };
  }

  if (item.toolType === "mcpToolCall") {
    const toolName = toolNameFromTitle(item.title);
    const args = parseToolArgs(item.detail);
    if (toolName.toLowerCase().includes("search")) {
      return {
        label: "searched",
        value:
          firstStringField(args, ["query", "pattern", "text"]) || item.detail,
      };
    }
    if (toolName.toLowerCase().includes("read")) {
      const targetPath =
        firstStringField(args, ["path", "file", "filename"]) || item.detail;
      return {
        label: "read",
        value: basename(targetPath),
        detail: targetPath && targetPath !== basename(targetPath) ? targetPath : "",
      };
    }
    if (toolName) {
      return {
        label: "tool",
        value: toolName,
        detail: item.detail || "",
      };
    }
  }

  return {
    label: "tool",
    value: item.title || "",
    detail: item.detail || "",
    output: item.output || "",
  };
}

function cleanCommandText(commandText: string) {
  if (!commandText) {
    return "";
  }
  const trimmed = commandText.trim();
  const shellMatch = trimmed.match(
    /^(?:\/\S+\/)?(?:bash|zsh|sh|fish)(?:\.exe)?\s+-lc\s+(['"])([\s\S]+)\1$/,
  );
  const inner = shellMatch ? shellMatch[2] : trimmed;
  const cdMatch = inner.match(
    /^\s*cd\s+[^&;]+(?:\s*&&\s*|\s*;\s*)([\s\S]+)$/i,
  );
  const stripped = cdMatch ? cdMatch[1] : inner;
  return stripped.trim();
}

function statusToneFromText(status?: string): StatusTone {
  if (!status) {
    return "unknown";
  }
  const normalized = status.toLowerCase();
  if (/(fail|error)/.test(normalized)) {
    return "failed";
  }
  if (/(pending|running|processing|started|in_progress)/.test(normalized)) {
    return "processing";
  }
  if (/(complete|completed|success|done)/.test(normalized)) {
    return "completed";
  }
  return "unknown";
}

function toolStatusTone(
  item: Extract<ConversationItem, { kind: "tool" }>,
  hasChanges: boolean,
): StatusTone {
  const fromStatus = statusToneFromText(item.status);
  if (fromStatus !== "unknown") {
    return fromStatus;
  }
  if (item.output || hasChanges) {
    return "completed";
  }
  return "processing";
}

function scrollKeyForItems(items: ConversationItem[]) {
  if (!items.length) {
    return "empty";
  }
  const last = items[items.length - 1];
  switch (last.kind) {
    case "message":
      return `${last.id}-${last.text.length}`;
    case "reasoning":
      return `${last.id}-${last.summary.length}-${last.content.length}`;
    case "tool":
      return `${last.id}-${last.status ?? ""}-${last.output?.length ?? 0}`;
    case "diff":
      return `${last.id}-${last.status ?? ""}-${last.diff.length}`;
    case "review":
      return `${last.id}-${last.state}-${last.text.length}`;
  }
}

export const Messages = memo(function Messages({
  items,
  isThinking,
  processingStartedAt = null,
  lastDurationMs = null,
  onToolPreview,
}: MessagesProps) {
  const bottomRef = useRef<HTMLDivElement | null>(null);
  const [expandedItems, setExpandedItems] = useState<Set<string>>(new Set());
  const [elapsedMs, setElapsedMs] = useState(0);
  const scrollKey = scrollKeyForItems(items);
  const toggleExpanded = (id: string) => {
    setExpandedItems((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };
  
  const handleToolClick = (item: Extract<ConversationItem, { kind: "tool" }>) => {
    toggleExpanded(item.id);
    
    // Trigger preview in sidebar if we have content to show
    if (!onToolPreview) return;
    
    const commandText = item.title.replace(/^Command:\s*/i, "").trim();
    const isReadTool = commandText.toLowerCase().startsWith('read ');
    const isEditOrWrite = item.toolType === 'fileChange' && item.changes?.length;
    
    if (isReadTool && item.output) {
      // Extract path from "read /path/to/file"
      const pathMatch = commandText.match(/^read\s+(.+)$/i);
      const path = pathMatch?.[1] || '';
      onToolPreview({
        type: 'read',
        path,
        content: item.output,
        language: languageFromPath(path) ?? undefined,
      });
    } else if (isEditOrWrite) {
      const change = item.changes?.[0];
      if (change?.diff) {
        onToolPreview({
          type: change.kind === 'create' ? 'write' : 'edit',
          path: change.path,
          diff: change.diff,
          language: languageFromPath(change.path) ?? undefined,
        });
      }
    }
  };

  const visibleItems = items;

  useEffect(() => {
    if (!bottomRef.current) {
      return undefined;
    }
    let raf1 = 0;
    let raf2 = 0;
    const target = bottomRef.current;
    raf1 = window.requestAnimationFrame(() => {
      raf2 = window.requestAnimationFrame(() => {
        target.scrollIntoView({ behavior: "smooth", block: "end" });
      });
    });
    return () => {
      if (raf1) {
        window.cancelAnimationFrame(raf1);
      }
      if (raf2) {
        window.cancelAnimationFrame(raf2);
      }
    };
  }, [scrollKey, isThinking]);

  useEffect(() => {
    if (!isThinking || !processingStartedAt) {
      setElapsedMs(0);
      return undefined;
    }
    setElapsedMs(Date.now() - processingStartedAt);
    const interval = window.setInterval(() => {
      setElapsedMs(Date.now() - processingStartedAt);
    }, 1000);
    return () => window.clearInterval(interval);
  }, [isThinking, processingStartedAt]);

  const elapsedSeconds = Math.max(0, Math.floor(elapsedMs / 1000));
  const elapsedMinutes = Math.floor(elapsedSeconds / 60);
  const elapsedRemainder = elapsedSeconds % 60;
  const formattedElapsed = `${elapsedMinutes}:${String(elapsedRemainder).padStart(2, "0")}`;
  const lastDurationSeconds = lastDurationMs
    ? Math.max(0, Math.floor(lastDurationMs / 1000))
    : 0;
  const lastDurationMinutes = Math.floor(lastDurationSeconds / 60);
  const lastDurationRemainder = lastDurationSeconds % 60;
  const formattedLastDuration = `${lastDurationMinutes}:${String(
    lastDurationRemainder,
  ).padStart(2, "0")}`;

  return (
    <div
      className="messages messages-full"
    >
      {visibleItems.map((item) => {
        if (item.kind === "message") {
          // Skip empty messages
          if (!item.text || !item.text.trim()) {
            return null;
          }
          return (
            <div key={item.id} className={`message ${item.role}`}>
              <div className="bubble">
                <Markdown value={item.text} className="markdown" />
              </div>
            </div>
          );
        }
        if (item.kind === "reasoning") {
          const fullContent = item.content || item.summary || "";
          const isExpanded = expandedItems.has(item.id);
          const reasoningTone: StatusTone = fullContent ? "completed" : "processing";
          
          // Create truncated preview (first 80 chars of first line)
          const firstLine = fullContent.split("\n")[0] || "";
          const previewText = firstLine.length > 80 
            ? `${firstLine.slice(0, 80)}…` 
            : firstLine;
          
          return (
            <div key={item.id} className={`tool-inline reasoning-inline ${isExpanded ? "tool-inline-expanded" : ""}`}>
              <button
                type="button"
                className="tool-inline-bar-toggle"
                onClick={() => toggleExpanded(item.id)}
                aria-expanded={isExpanded}
                aria-label="Toggle thinking details"
              />
              <div className="tool-inline-content">
                <button
                  type="button"
                  className="tool-inline-summary tool-inline-toggle"
                  onClick={() => toggleExpanded(item.id)}
                  aria-expanded={isExpanded}
                >
                  <span
                    className={`tool-inline-dot ${reasoningTone}`}
                    aria-hidden
                  />
                  <span className="tool-inline-value">
                    {isExpanded ? "Thinking" : (previewText || "Thinking")}
                  </span>
                </button>
                {isExpanded && fullContent && (
                  <div className="reasoning-inline-detail markdown">
                    <Markdown value={fullContent} />
                  </div>
                )}
              </div>
            </div>
          );
        }
        if (item.kind === "review") {
          const title =
            item.state === "started" ? "Review started" : "Review completed";
          return (
            <div key={item.id} className="item-card review">
              <div className="review-header">
                <span className="review-title">{title}</span>
                <span
                  className={`review-badge ${
                    item.state === "started" ? "active" : "done"
                  }`}
                >
                  Review
                </span>
              </div>
              {item.text && (
                <Markdown value={item.text} className="item-text markdown" />
              )}
            </div>
          );
        }
        if (item.kind === "diff") {
          return (
            <div key={item.id} className="item-card diff">
              <div className="diff-header">
                <span className="diff-title">{item.title}</span>
                {item.status && <span className="item-status">{item.status}</span>}
              </div>
              <div className="diff-viewer-output">
                <DiffBlock diff={item.diff} language={languageFromPath(item.title)} />
              </div>
            </div>
          );
        }
        if (item.kind === "tool") {
          const isFileChange = item.toolType === "fileChange";
          const isCommand = item.toolType === "commandExecution";
          const commandText = isCommand
            ? item.title.replace(/^Command:\s*/i, "").trim()
            : "";
          const summary = buildToolSummary(item, commandText);
          const changeNames = (item.changes ?? [])
            .map((change) => basename(change.path))
            .filter(Boolean);
          const hasChanges = changeNames.length > 0;
          const tone = toolStatusTone(item, hasChanges);
          const isExpanded = expandedItems.has(item.id);
          const summaryLabel = isFileChange
            ? changeNames.length > 1
              ? "files edited"
              : "file edited"
            : isCommand
              ? ""
            : summary.label;
          const summaryValue = isFileChange
            ? changeNames.length > 1
              ? `${changeNames[0]} +${changeNames.length - 1}`
              : changeNames[0] || "changes"
            : summary.value;
          const shouldFadeCommand =
            isCommand && !isExpanded && (summaryValue?.length ?? 0) > 80;
          return (
            <div
              key={item.id}
              className={`tool-inline ${
                expandedItems.has(item.id) ? "tool-inline-expanded" : ""
              }`}
            >
              <button
                type="button"
                className="tool-inline-bar-toggle"
                onClick={() => handleToolClick(item)}
                aria-expanded={expandedItems.has(item.id)}
                aria-label="Toggle tool details"
              />
              <div className="tool-inline-content">
                <button
                  type="button"
                  className="tool-inline-summary tool-inline-toggle"
                  onClick={() => handleToolClick(item)}
                  aria-expanded={expandedItems.has(item.id)}
                >
                  <span className={`tool-inline-dot ${tone}`} aria-hidden />
                  {summaryLabel && (
                    <span className="tool-inline-label">{summaryLabel}:</span>
                  )}
                  {summaryValue && (
                    <span
                      className={`tool-inline-value ${
                        isCommand ? "tool-inline-command" : ""
                      } ${isCommand && isExpanded ? "tool-inline-command-full" : ""}`}
                    >
                      {isCommand ? (
                        <span
                          className={`tool-inline-command-text ${
                            shouldFadeCommand ? "tool-inline-command-fade" : ""
                          }`}
                        >
                          {summaryValue}
                        </span>
                      ) : (
                        summaryValue
                      )}
                    </span>
                  )}
                </button>
                {isExpanded && summary.detail && !isFileChange && (
                  <div className="tool-inline-detail">
                    {summary.detail}
                  </div>
                )}
                {isExpanded && isCommand && item.detail && (
                  <div className="tool-inline-detail tool-inline-muted">
                    cwd: {item.detail}
                  </div>
                )}
                {isExpanded && isFileChange && hasChanges && (
                  <div className="tool-inline-change-list">
                    {item.changes?.map((change, index) => (
                      <div
                        key={`${change.path}-${index}`}
                        className="tool-inline-change"
                      >
                        <div className="tool-inline-change-header">
                          {change.kind && (
                            <span className="tool-inline-change-kind">
                              {change.kind.toUpperCase()}
                            </span>
                          )}
                          <span className="tool-inline-change-path">
                            {basename(change.path)}
                          </span>
                        </div>
                        {change.diff && (
                          <div className="diff-viewer-output">
                            <DiffBlock
                              diff={change.diff}
                              language={languageFromPath(change.path)}
                            />
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}
                {isExpanded && isFileChange && !hasChanges && item.detail && (
                  <Markdown value={item.detail} className="item-text markdown" />
                )}
                {isExpanded && summary.output && (!isFileChange || !hasChanges) && (
                  <CodeBlock
                    code={summary.output}
                    filePath={summary.value || item.detail || item.title}
                    showLineNumbers={summary.output.split('\n').length > 1}
                    className="tool-inline-output"
                  />
                )}
              </div>
            </div>
          );
        }
        return null;
      })}
      {isThinking && (
        <div className="working">
          <span className="working-spinner" aria-hidden />
          <div className="working-timer">
            <span className="working-timer-clock">{formattedElapsed}</span>
          </div>
          <span className="working-text">Working…</span>
        </div>
      )}
      {!isThinking && lastDurationMs !== null && items.length > 0 && (
        <div className="turn-complete" aria-live="polite">
          <span className="turn-complete-line" aria-hidden />
          <span className="turn-complete-label">
            Done in {formattedLastDuration}
          </span>
          <span className="turn-complete-line" aria-hidden />
        </div>
      )}
      {!items.length && (
        <div className="empty messages-empty">
          Start a thread and send a prompt to the agent.
        </div>
      )}
      <div ref={bottomRef} />
    </div>
  );
});
