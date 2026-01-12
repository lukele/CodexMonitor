import type { WorkspaceInfo } from "../types";

type MainHeaderProps = {
  workspace: WorkspaceInfo;
  branchName: string;
};

export function MainHeader({ workspace, branchName }: MainHeaderProps) {
  return (
    <header className="main-header" data-tauri-drag-region>
      <div className="workspace-header">
        <div className="workspace-title-line">
          <span className="workspace-title">{workspace.name}</span>
          <span className="workspace-separator" aria-hidden>
            ›
          </span>
          <span className="workspace-branch">{branchName}</span>
        </div>
      </div>
    </header>
  );
}
