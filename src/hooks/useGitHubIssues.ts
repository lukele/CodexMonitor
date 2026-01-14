import { useCallback, useEffect, useRef, useState } from "react";
import type { GitHubIssue, WorkspaceInfo } from "../types";
import { getGitHubIssues } from "../services/tauri";

type GitHubIssuesState = {
  issues: GitHubIssue[];
  isLoading: boolean;
  error: string | null;
};

const emptyState: GitHubIssuesState = {
  issues: [],
  isLoading: false,
  error: null,
};

export function useGitHubIssues(
  activeWorkspace: WorkspaceInfo | null,
  enabled: boolean,
) {
  const [state, setState] = useState<GitHubIssuesState>(emptyState);
  const requestIdRef = useRef(0);
  const workspaceIdRef = useRef<string | null>(activeWorkspace?.id ?? null);

  const refresh = useCallback(async () => {
    if (!activeWorkspace) {
      setState(emptyState);
      return;
    }
    const workspaceId = activeWorkspace.id;
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setState((prev) => ({ ...prev, isLoading: true, error: null }));
    try {
      const issues = await getGitHubIssues(workspaceId);
      if (
        requestIdRef.current !== requestId ||
        workspaceIdRef.current !== workspaceId
      ) {
        return;
      }
      setState({
        issues,
        isLoading: false,
        error: null,
      });
    } catch (error) {
      console.error("Failed to load GitHub issues", error);
      if (
        requestIdRef.current !== requestId ||
        workspaceIdRef.current !== workspaceId
      ) {
        return;
      }
      setState({
        issues: [],
        isLoading: false,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }, [activeWorkspace]);

  useEffect(() => {
    const workspaceId = activeWorkspace?.id ?? null;
    if (workspaceIdRef.current !== workspaceId) {
      workspaceIdRef.current = workspaceId;
      requestIdRef.current += 1;
      setState(emptyState);
    }
  }, [activeWorkspace?.id]);

  useEffect(() => {
    if (!enabled) {
      return;
    }
    void refresh();
  }, [enabled, refresh]);

  return {
    issues: state.issues,
    isLoading: state.isLoading,
    error: state.error,
    refresh,
  };
}
