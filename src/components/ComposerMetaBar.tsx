import type { CSSProperties } from "react";
import type { AccessMode, Backend, ThreadTokenUsage } from "../types";

type ModelInfo = {
  id: string;
  displayName: string;
  model: string;
  backend?: Backend;
  provider?: string;
};

type ComposerMetaBarProps = {
  disabled: boolean;
  models: ModelInfo[];
  selectedModelId: string | null;
  onSelectModel: (id: string) => void;
  reasoningOptions: string[];
  selectedEffort: string | null;
  onSelectEffort: (effort: string) => void;
  accessMode: AccessMode;
  onSelectAccessMode: (mode: AccessMode) => void;
  contextUsage?: ThreadTokenUsage | null;
  currentBackend?: Backend;
  isSwitchingBackend?: boolean;
};

export function ComposerMetaBar({
  disabled,
  models,
  selectedModelId,
  onSelectModel,
  reasoningOptions,
  selectedEffort,
  onSelectEffort,
  accessMode,
  onSelectAccessMode,
  contextUsage = null,
  currentBackend = "codex",
  isSwitchingBackend = false,
}: ComposerMetaBarProps) {
  const contextWindow = contextUsage?.modelContextWindow ?? null;
  const lastTokens = contextUsage?.last.totalTokens ?? 0;
  const totalTokens = contextUsage?.total.totalTokens ?? 0;
  const usedTokens = lastTokens > 0 ? lastTokens : totalTokens;
  const contextFreePercent =
    contextWindow && contextWindow > 0 && usedTokens > 0
      ? Math.max(
          0,
          100 -
            Math.min(Math.max((usedTokens / contextWindow) * 100, 0), 100),
        )
      : null;

  // Group models by provider (inferred from model name or provider field)
  const getProvider = (m: ModelInfo) => {
    if (m.provider) return m.provider;
    if (m.model.startsWith("claude-")) return "anthropic";
    if (m.model.startsWith("gpt-") || m.model.startsWith("o1-") || m.model.startsWith("o3-")) return "openai";
    if (m.model.startsWith("gemini-")) return "google";
    if (m.model.startsWith("mistral") || m.model.startsWith("codestral")) return "mistral";
    if (m.model.startsWith("grok") || m.model.includes("pickle") || m.model.includes("glm")) return "opencode";
    return "other";
  };
  
  const providerGroups = models.reduce((acc, m) => {
    const provider = getProvider(m);
    if (!acc[provider]) acc[provider] = [];
    acc[provider].push(m);
    return acc;
  }, {} as Record<string, ModelInfo[]>);
  
  const providerLabels: Record<string, string> = {
    anthropic: "🟣 Anthropic",
    openai: "🟢 OpenAI",
    google: "🔵 Google",
    mistral: "🟠 Mistral",
    opencode: "⚡ OpenCode",
    "google-antigravity": "☁️ Google Antigravity",
    "opencode-zen": "⚡ OpenCode Zen",
    other: "📦 Other",
  };

  // Backend icon
  const BackendIcon = currentBackend === "claude" ? (
    // Claude icon (simplified anthropic logo)
    <svg viewBox="0 0 24 24" fill="none">
      <path
        d="M12 3L4 7v10l8 4 8-4V7l-8-4z"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
      <path
        d="M12 7v10"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      <path
        d="M8 9l4 2 4-2"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  ) : (
    // Codex/OpenAI icon
    <svg viewBox="0 0 24 24" fill="none">
      <path
        d="M7 8V6a5 5 0 0 1 10 0v2"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      <rect
        x="4.5"
        y="8"
        width="15"
        height="11"
        rx="3"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <circle cx="9" cy="13" r="1" fill="currentColor" />
      <circle cx="15" cy="13" r="1" fill="currentColor" />
      <path
        d="M9 16h6"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
    </svg>
  );

  return (
    <div className="composer-bar">
      <div className="composer-meta">
        <div className="composer-select-wrap">
          <span 
            className={`composer-icon ${isSwitchingBackend ? 'composer-icon--switching' : ''}`} 
            aria-hidden
            title={`Backend: ${currentBackend === 'claude' ? 'Anthropic Claude' : 'OpenAI Codex'}`}
          >
            {BackendIcon}
          </span>
          <select
            className="composer-select composer-select--model"
            aria-label="Model"
            value={selectedModelId ?? ""}
            onChange={(event) => onSelectModel(event.target.value)}
            disabled={disabled || isSwitchingBackend}
          >
            {models.length === 0 && <option value="">No models</option>}
            {Object.entries(providerGroups).map(([provider, providerModels]) => (
              <optgroup key={provider} label={providerLabels[provider] || provider}>
                {providerModels.map((model, idx) => (
                  <option key={`${provider}-${model.id}-${idx}`} value={model.id}>
                    {model.displayName || model.model}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          {isSwitchingBackend && (
            <span className="composer-switching-indicator">Switching...</span>
          )}
        </div>
        <div className="composer-select-wrap">
          <span className="composer-icon" aria-hidden>
            <svg viewBox="0 0 24 24" fill="none">
              <path
                d="M8.5 4.5a3.5 3.5 0 0 0-3.46 4.03A4 4 0 0 0 6 16.5h2"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
              <path
                d="M15.5 4.5a3.5 3.5 0 0 1 3.46 4.03A4 4 0 0 1 18 16.5h-2"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
              <path
                d="M9 12h6"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
              <path
                d="M12 12v6"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
            </svg>
          </span>
          <select
            className="composer-select composer-select--effort"
            aria-label="Thinking mode"
            value={selectedEffort ?? ""}
            onChange={(event) => onSelectEffort(event.target.value)}
            disabled={disabled || isSwitchingBackend}
          >
            {reasoningOptions.length === 0 && <option value="">Default</option>}
            {reasoningOptions.map((effort) => (
              <option key={effort} value={effort}>
                {effort}
              </option>
            ))}
          </select>
        </div>
        <div className="composer-select-wrap">
          <span className="composer-icon" aria-hidden>
            <svg viewBox="0 0 24 24" fill="none">
              <path
                d="M12 4l7 3v5c0 4.5-3 7.5-7 8-4-0.5-7-3.5-7-8V7l7-3z"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinejoin="round"
              />
              <path
                d="M9.5 12.5l1.8 1.8 3.7-4"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </span>
          <select
            className="composer-select composer-select--approval"
            aria-label="Agent access"
            disabled={disabled || isSwitchingBackend}
            value={accessMode}
            onChange={(event) =>
              onSelectAccessMode(event.target.value as AccessMode)
            }
          >
            <option value="read-only">Read only</option>
            <option value="current">On-Request</option>
            <option value="full-access">Full access</option>
          </select>
        </div>
      </div>
      <div className="composer-context">
        <div
          className="composer-context-ring"
          data-tooltip={
            contextFreePercent === null
              ? "Context free --"
              : `Context free ${Math.round(contextFreePercent)}%`
          }
          aria-label={
            contextFreePercent === null
              ? "Context free --"
              : `Context free ${Math.round(contextFreePercent)}%`
          }
          style={
            {
              "--context-free": contextFreePercent ?? 0,
            } as CSSProperties
          }
        >
          <span className="composer-context-value">●</span>
        </div>
      </div>
    </div>
  );
}
