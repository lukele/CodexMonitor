import type { ToolPreview } from "./Messages";
import { DiffBlock } from "./DiffBlock";
import { highlightLine } from "../utils/syntax";
import { useMemo } from "react";
import { FileText, FileDiff, FileCode, X } from "lucide-react";

type ToolPreviewPanelProps = {
  preview: ToolPreview | null;
  onClose?: () => void;
};

export function ToolPreviewPanel({ preview, onClose }: ToolPreviewPanelProps) {
  const highlightedLines = useMemo(() => {
    if (!preview?.content) {
      return null;
    }
    const lines = preview.content.split('\n');
    return lines.map(line => highlightLine(line, preview.language));
  }, [preview?.content, preview?.language]);

  if (!preview) {
    return (
      <div className="tool-preview-panel tool-preview-empty">
        <div className="tool-preview-placeholder">
          <FileText className="tool-preview-placeholder-icon" />
          <p>Click on a tool result to preview its content here.</p>
        </div>
      </div>
    );
  }

  const fileName = preview.path?.split("/").pop() || "File";
  const Icon = preview.type === "read" ? FileCode : FileDiff;

  return (
    <div className="tool-preview-panel">
      <div className="tool-preview-header">
        <div className="tool-preview-title">
          <Icon className="tool-preview-icon" size={14} />
          <span className="tool-preview-type">
            {preview.type === "read" ? "Read" : preview.type === "write" ? "Write" : "Edit"}
          </span>
          <span className="tool-preview-path" title={preview.path}>
            {fileName}
          </span>
        </div>
        {onClose && (
          <button
            type="button"
            className="tool-preview-close"
            onClick={onClose}
            aria-label="Close preview"
          >
            <X size={14} />
          </button>
        )}
      </div>
      <div className="tool-preview-content">
        {preview.type === "read" && preview.content && (
          <div className="tool-preview-code">
            <pre>
              <code>
                {highlightedLines ? (
                  highlightedLines.map((line, i) => (
                    <div key={i} className="tool-preview-line">
                      <span className="tool-preview-line-number">{i + 1}</span>
                      <span 
                        className="tool-preview-line-content"
                        dangerouslySetInnerHTML={{ __html: line || '&nbsp;' }}
                      />
                    </div>
                  ))
                ) : (
                  preview.content.split('\n').map((line, i) => (
                    <div key={i} className="tool-preview-line">
                      <span className="tool-preview-line-number">{i + 1}</span>
                      <span className="tool-preview-line-content">{line || '\u00A0'}</span>
                    </div>
                  ))
                )}
              </code>
            </pre>
          </div>
        )}
        {(preview.type === "edit" || preview.type === "write") && preview.diff && (
          <div className="tool-preview-diff">
            <DiffBlock diff={preview.diff} language={preview.language} />
          </div>
        )}
        {!preview.content && !preview.diff && (
          <div className="tool-preview-no-content">
            No content available for preview.
          </div>
        )}
      </div>
    </div>
  );
}
