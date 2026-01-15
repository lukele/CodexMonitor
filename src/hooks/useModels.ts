import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Backend, DebugEntry, ModelOption, WorkspaceInfo } from "../types";
import { getAllModels, getModelList, switchBackend } from "../services/tauri";

type UseModelsOptions = {
  activeWorkspace: WorkspaceInfo | null;
  onDebug?: (entry: DebugEntry) => void;
  onBackendSwitch?: (backend: Backend) => void;
};

const STORAGE_KEY_MODEL = "codex-monitor-selected-model";
const STORAGE_KEY_EFFORT = "codex-monitor-selected-effort";

export function useModels({ activeWorkspace, onDebug, onBackendSwitch }: UseModelsOptions) {
  const [models, setModels] = useState<ModelOption[]>([]);
  const [selectedModelId, setSelectedModelId] = useState<string | null>(() => {
    try {
      return localStorage.getItem(STORAGE_KEY_MODEL);
    } catch {
      return null;
    }
  });
  const [selectedEffort, setSelectedEffort] = useState<string | null>(() => {
    try {
      return localStorage.getItem(STORAGE_KEY_EFFORT);
    } catch {
      return null;
    }
  });
  const [isSwitchingBackend, setIsSwitchingBackend] = useState(false);
  const lastFetchedWorkspaceId = useRef<string | null>(null);
  const inFlight = useRef(false);
  
  // Persist selected model to localStorage
  useEffect(() => {
    try {
      if (selectedModelId) {
        localStorage.setItem(STORAGE_KEY_MODEL, selectedModelId);
      }
    } catch {
      // ignore
    }
  }, [selectedModelId]);
  
  // Persist selected effort to localStorage
  useEffect(() => {
    try {
      if (selectedEffort) {
        localStorage.setItem(STORAGE_KEY_EFFORT, selectedEffort);
      }
    } catch {
      // ignore
    }
  }, [selectedEffort]);

  const workspaceId = activeWorkspace?.id ?? null;
  const isConnected = Boolean(activeWorkspace?.connected);
  const currentBackend = activeWorkspace?.backend ?? "codex";

  const selectedModel = useMemo(
    () => models.find((model) => model.id === selectedModelId) ?? null,
    [models, selectedModelId],
  );

  const reasoningOptions = useMemo(() => {
    if (!selectedModel) {
      return [];
    }
    return selectedModel.supportedReasoningEfforts.map(
      (effort) => effort.reasoningEffort,
    );
  }, [selectedModel]);

  // Fetch all models (combined list from both backends)
  const refreshModels = useCallback(async () => {
    if (!workspaceId || !isConnected) {
      return;
    }
    if (inFlight.current) {
      return;
    }
    inFlight.current = true;
    onDebug?.({
      id: `${Date.now()}-client-model-list`,
      timestamp: Date.now(),
      source: "client",
      label: "get_all_models",
      payload: { workspaceId },
    });
    try {
      // Get combined model list (static list of all available models)
      const allModelsResponse = await getAllModels();
      onDebug?.({
        id: `${Date.now()}-server-all-models`,
        timestamp: Date.now(),
        source: "server",
        label: "get_all_models response",
        payload: allModelsResponse,
      });
      
      const rawData = allModelsResponse.data ?? [];
      const data: ModelOption[] = rawData.map((item: any) => ({
        id: String(item.id ?? item.model ?? ""),
        model: String(item.model ?? item.id ?? ""),
        displayName: String(item.displayName ?? item.display_name ?? item.model ?? ""),
        description: String(item.description ?? ""),
        supportedReasoningEfforts: Array.isArray(item.supportedReasoningEfforts)
          ? item.supportedReasoningEfforts
          : Array.isArray(item.supported_reasoning_efforts)
            ? item.supported_reasoning_efforts.map((effort: any) => ({
                reasoningEffort: String(
                  effort.reasoningEffort ?? effort.reasoning_effort ?? "",
                ),
                description: String(effort.description ?? ""),
              }))
            : [],
        defaultReasoningEffort: String(
          item.defaultReasoningEffort ?? item.default_reasoning_effort ?? "",
        ),
        isDefault: Boolean(item.isDefault ?? item.is_default ?? false),
        backend: item.backend as Backend | undefined,
      }));
      
      setModels(data);
      lastFetchedWorkspaceId.current = workspaceId;
      
      // Select default model based on current backend
      const backendModels = data.filter(m => m.backend === currentBackend);
      const defaultModel = backendModels.find((model) => model.isDefault) 
        ?? backendModels[0] 
        ?? data.find((model) => model.isDefault) 
        ?? data[0] 
        ?? null;
      
      if (defaultModel && !selectedModelId) {
        setSelectedModelId(defaultModel.id);
        setSelectedEffort(defaultModel.defaultReasoningEffort ?? null);
      }
    } catch (error) {
      onDebug?.({
        id: `${Date.now()}-client-model-list-error`,
        timestamp: Date.now(),
        source: "error",
        label: "get_all_models error",
        payload: error instanceof Error ? error.message : String(error),
      });
      
      // Fallback: try to get models from current backend
      try {
        const response = await getModelList(workspaceId);
        const rawData = response.result?.data ?? response.data ?? [];
        const data: ModelOption[] = rawData.map((item: any) => ({
          id: String(item.id ?? item.model ?? ""),
          model: String(item.model ?? item.id ?? ""),
          displayName: String(item.displayName ?? item.display_name ?? item.model ?? ""),
          description: String(item.description ?? ""),
          supportedReasoningEfforts: Array.isArray(item.supportedReasoningEfforts)
            ? item.supportedReasoningEfforts
            : [],
          defaultReasoningEffort: String(item.defaultReasoningEffort ?? ""),
          isDefault: Boolean(item.isDefault ?? false),
          backend: currentBackend,
        }));
        setModels(data);
        lastFetchedWorkspaceId.current = workspaceId;
        const defaultModel = data.find((model) => model.isDefault) ?? data[0] ?? null;
        if (defaultModel && !selectedModelId) {
          setSelectedModelId(defaultModel.id);
          setSelectedEffort(defaultModel.defaultReasoningEffort ?? null);
        }
      } catch {
        // Ignore fallback errors
      }
    } finally {
      inFlight.current = false;
    }
  }, [currentBackend, isConnected, onDebug, selectedModelId, workspaceId]);

  // Handle model selection - switch backend if needed
  const handleModelSelect = useCallback(async (modelId: string) => {
    const model = models.find(m => m.id === modelId);
    if (!model || !workspaceId) {
      setSelectedModelId(modelId);
      return;
    }

    const modelBackend = model.backend ?? (model.model.startsWith("claude-") ? "claude" : "codex");
    
    // Check if we need to switch backends
    if (modelBackend !== currentBackend) {
      setIsSwitchingBackend(true);
      onDebug?.({
        id: `${Date.now()}-backend-switch`,
        timestamp: Date.now(),
        source: "client",
        label: "switch_backend",
        payload: { from: currentBackend, to: modelBackend, model: model.model },
      });

      try {
        const result = await switchBackend(workspaceId, modelBackend);
        onDebug?.({
          id: `${Date.now()}-backend-switch-result`,
          timestamp: Date.now(),
          source: "server",
          label: "switch_backend result",
          payload: result,
        });

        if (result.switched) {
          onBackendSwitch?.(modelBackend);
        }
      } catch (error) {
        onDebug?.({
          id: `${Date.now()}-backend-switch-error`,
          timestamp: Date.now(),
          source: "error",
          label: "switch_backend error",
          payload: error instanceof Error ? error.message : String(error),
        });
        // Don't change the model if backend switch failed
        setIsSwitchingBackend(false);
        return;
      }
      setIsSwitchingBackend(false);
    }

    setSelectedModelId(modelId);
    setSelectedEffort(model.defaultReasoningEffort ?? null);
  }, [currentBackend, models, onBackendSwitch, onDebug, workspaceId]);

  useEffect(() => {
    if (!workspaceId || !isConnected) {
      return;
    }
    if (lastFetchedWorkspaceId.current === workspaceId && models.length > 0) {
      return;
    }
    refreshModels();
  }, [isConnected, models.length, refreshModels, workspaceId]);

  useEffect(() => {
    if (!selectedModel) {
      return;
    }
    if (
      selectedEffort &&
      selectedModel.supportedReasoningEfforts.some(
        (effort) => effort.reasoningEffort === selectedEffort,
      )
    ) {
      return;
    }
    setSelectedEffort(selectedModel.defaultReasoningEffort ?? null);
  }, [selectedEffort, selectedModel]);

  return {
    models,
    selectedModel,
    selectedModelId,
    setSelectedModelId: handleModelSelect,
    reasoningOptions,
    selectedEffort,
    setSelectedEffort,
    refreshModels,
    currentBackend,
    isSwitchingBackend,
  };
}
