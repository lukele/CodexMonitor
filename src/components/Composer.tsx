import { useCallback, useEffect, useRef, useState } from "react";
import type { CustomPromptOption, QueuedMessage, ThreadTokenUsage } from "../types";
import { useComposerAutocompleteState } from "../hooks/useComposerAutocompleteState";
import { ComposerInput } from "./ComposerInput";
import { ComposerMetaBar } from "./ComposerMetaBar";
import { ComposerQueue } from "./ComposerQueue";

type ComposerProps = {
  onSend: (text: string, images: string[]) => void;
  onStop: () => void;
  canStop: boolean;
  disabled?: boolean;
  models: { id: string; displayName: string; model: string }[];
  selectedModelId: string | null;
  onSelectModel: (id: string) => void;
  reasoningOptions: string[];
  selectedEffort: string | null;
  onSelectEffort: (effort: string) => void;
  accessMode: "read-only" | "current" | "full-access";
  onSelectAccessMode: (mode: "read-only" | "current" | "full-access") => void;
  skills: { name: string; description?: string }[];
  prompts: CustomPromptOption[];
  files: string[];
  contextUsage?: ThreadTokenUsage | null;
  queuedMessages?: QueuedMessage[];
  onEditQueued?: (item: QueuedMessage) => void;
  onDeleteQueued?: (id: string) => void;
  sendLabel?: string;
  draftText?: string;
  onDraftChange?: (text: string) => void;
  attachedImages?: string[];
  onPickImages?: () => void;
  onAttachImages?: (paths: string[]) => void;
  onRemoveImage?: (path: string) => void;
  prefillDraft?: QueuedMessage | null;
  onPrefillHandled?: (id: string) => void;
  insertText?: QueuedMessage | null;
  onInsertHandled?: (id: string) => void;
  textareaRef?: React.RefObject<HTMLTextAreaElement | null>;
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
  prompts,
  files,
  contextUsage = null,
  queuedMessages = [],
  onEditQueued,
  onDeleteQueued,
  sendLabel = "Send",
  draftText = "",
  onDraftChange,
  attachedImages = [],
  onPickImages,
  onAttachImages,
  onRemoveImage,
  prefillDraft = null,
  onPrefillHandled,
  insertText = null,
  onInsertHandled,
  textareaRef: externalTextareaRef,
}: ComposerProps) {
  const [text, setText] = useState(draftText);
  const [selectionStart, setSelectionStart] = useState<number | null>(null);
  const internalRef = useRef<HTMLTextAreaElement | null>(null);
  const textareaRef = externalTextareaRef ?? internalRef;

  useEffect(() => {
    if (draftText === text) {
      return;
    }
    setText(draftText);
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
    if (!trimmed && attachedImages.length === 0) {
      return;
    }
    onSend(trimmed, attachedImages);
    setComposerText("");
  }, [attachedImages, disabled, onSend, setComposerText, text]);

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
    prompts,
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
        attachments={attachedImages}
        onAddAttachment={onPickImages}
        onAttachImages={onAttachImages}
        onRemoveAttachment={onRemoveImage}
        onTextChange={handleTextChange}
        onSelectionChange={handleSelectionChange}
        onKeyDown={(event) => {
          if (event.key === "Enter" && event.shiftKey) {
            event.preventDefault();
            const textarea = textareaRef.current;
            if (!textarea) {
              return;
            }
            const start = textarea.selectionStart ?? text.length;
            const end = textarea.selectionEnd ?? start;
            const nextText = `${text.slice(0, start)}\n${text.slice(end)}`;
            const nextCursor = start + 1;
            setComposerText(nextText);
            requestAnimationFrame(() => {
              textarea.focus();
              textarea.setSelectionRange(nextCursor, nextCursor);
              handleSelectionChange(nextCursor);
            });
            return;
          }
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
        disabled={disabled}
        models={models}
        selectedModelId={selectedModelId}
        onSelectModel={onSelectModel}
        reasoningOptions={reasoningOptions}
        selectedEffort={selectedEffort}
        onSelectEffort={onSelectEffort}
        accessMode={accessMode}
        onSelectAccessMode={onSelectAccessMode}
        contextUsage={contextUsage}
      />
    </footer>
  );
}
