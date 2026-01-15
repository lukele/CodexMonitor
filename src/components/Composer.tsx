import { useCallback, useEffect, useRef, useState } from "react";
import type { Backend, QueuedMessage, ThreadTokenUsage } from "../types";
import { useComposerAutocompleteState } from "../hooks/useComposerAutocompleteState";
import { ComposerInput } from "./ComposerInput";
import { ComposerMetaBar } from "./ComposerMetaBar";
import { ComposerQueue } from "./ComposerQueue";

type ComposerProps = {
  onSend: (text: string) => void;
  onStop: () => void;
  canStop: boolean;
  disabled?: boolean;
  models: { id: string; displayName: string; model: string; backend?: Backend }[];
  selectedModelId: string | null;
  onSelectModel: (id: string) => void;
  reasoningOptions: string[];
  selectedEffort: string | null;
  onSelectEffort: (effort: string) => void;
  accessMode: "read-only" | "current" | "full-access";
  onSelectAccessMode: (mode: "read-only" | "current" | "full-access") => void;
  skills: { name: string; description?: string }[];
  files: string[];
  contextUsage?: ThreadTokenUsage | null;
  queuedMessages?: QueuedMessage[];
  onEditQueued?: (item: QueuedMessage) => void;
  onDeleteQueued?: (id: string) => void;
  sendLabel?: string;
  draftText?: string;
  onDraftChange?: (text: string) => void;
  prefillDraft?: QueuedMessage | null;
  onPrefillHandled?: (id: string) => void;
  insertText?: QueuedMessage | null;
  onInsertHandled?: (id: string) => void;
  currentBackend?: Backend;
  isSwitchingBackend?: boolean;
};

export function Composer({
  onSend,
  onStop,
  canStop,
  disabled = false,
  models,
  selectedModelId,
  onSelectModel,
  reasoningOptions,
  selectedEffort,
  onSelectEffort,
  accessMode,
  onSelectAccessMode,
  skills,
  files,
  contextUsage = null,
  queuedMessages = [],
  onEditQueued,
  onDeleteQueued,
  sendLabel = "Send",
  draftText = "",
  onDraftChange,
  prefillDraft = null,
  onPrefillHandled,
  insertText = null,
  onInsertHandled,
  currentBackend = "codex",
  isSwitchingBackend = false,
}: ComposerProps) {
  const [text, setText] = useState(draftText);
  const [selectionStart, setSelectionStart] = useState<number | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const lastDraftRef = useRef(draftText);

  // Only sync text when draftText changes externally (not from our own typing)
  useEffect(() => {
    if (draftText !== lastDraftRef.current) {
      lastDraftRef.current = draftText;
      if (draftText !== text) {
        setText(draftText);
      }
    }
  }, [draftText, text]);

  const setComposerText = useCallback(
    (next: string) => {
      setText(next);
      onDraftChange?.(next);
    },
    [onDraftChange],
  );

  const handleSend = useCallback(() => {
    if (disabled) {
      return;
    }
    const trimmed = text.trim();
    if (!trimmed) {
      return;
    }
    onSend(trimmed);
    setComposerText("");
  }, [disabled, onSend, setComposerText, text]);

  const {
    isAutocompleteOpen,
    autocompleteMatches,
    highlightIndex,
    setHighlightIndex,
    applyAutocomplete,
    handleInputKeyDown,
    handleTextChange,
    handleSelectionChange,
  } = useComposerAutocompleteState({
    text,
    selectionStart,
    disabled,
    skills,
    files,
    textareaRef,
    setText: setComposerText,
    setSelectionStart,
  });

  useEffect(() => {
    if (!prefillDraft) {
      return;
    }
    setComposerText(prefillDraft.text);
    onPrefillHandled?.(prefillDraft.id);
  }, [prefillDraft, onPrefillHandled, setComposerText]);

  useEffect(() => {
    if (!insertText) {
      return;
    }
    setComposerText(insertText.text);
    onInsertHandled?.(insertText.id);
  }, [insertText, onInsertHandled, setComposerText]);

  return (
    <footer className={`composer${disabled ? " is-disabled" : ""}`}>
      <ComposerQueue
        queuedMessages={queuedMessages}
        onEditQueued={onEditQueued}
        onDeleteQueued={onDeleteQueued}
      />
      <ComposerInput
        text={text}
        disabled={disabled}
        sendLabel={sendLabel}
        canStop={canStop}
        onStop={onStop}
        onSend={handleSend}
        onTextChange={handleTextChange}
        onSelectionChange={handleSelectionChange}
        onKeyDown={(event) => {
          handleInputKeyDown(event);
          if (event.defaultPrevented) {
            return;
          }
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            handleSend();
          }
        }}
        textareaRef={textareaRef}
        suggestionsOpen={isAutocompleteOpen}
        suggestions={autocompleteMatches}
        highlightIndex={highlightIndex}
        onHighlightIndex={setHighlightIndex}
        onSelectSuggestion={applyAutocomplete}
      />
      <ComposerMetaBar
        disabled={disabled || isSwitchingBackend}
        models={models}
        selectedModelId={selectedModelId}
        onSelectModel={onSelectModel}
        reasoningOptions={reasoningOptions}
        selectedEffort={selectedEffort}
        onSelectEffort={onSelectEffort}
        accessMode={accessMode}
        onSelectAccessMode={onSelectAccessMode}
        contextUsage={contextUsage}
        currentBackend={currentBackend}
        isSwitchingBackend={isSwitchingBackend}
      />
    </footer>
  );
}
