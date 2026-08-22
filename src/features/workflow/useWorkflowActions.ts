import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { message } from "antd";

import { desktopBridge } from "../../bridge";
import type { PersistedJob, WorkflowIntentResult } from "../../domain";

export type WorkflowAction = "continue" | "retry" | "pause" | "cancel";

export function useWorkflowActions(onOpenProject: (projectId: string) => void) {
  const queryClient = useQueryClient();

  return useCallback(async (job: PersistedJob, action: WorkflowAction) => {
    let result: WorkflowIntentResult | null = null;
    if (action === "cancel") await desktopBridge.cancelWorkflow(job.id);
    else if (action === "pause") await desktopBridge.pauseWorkflow(job.id);
    else if (action === "retry") result = await desktopBridge.retryWorkflow(job.id);
    else result = await desktopBridge.continueWorkflow(job.id);

    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["jobs"] }),
      queryClient.invalidateQueries({ queryKey: ["projects"] }),
      queryClient.invalidateQueries({ queryKey: ["segments", job.projectId] }),
      queryClient.invalidateQueries({ queryKey: ["readiness", job.projectId] }),
    ]);
    if (result?.nextAction === "open_editor") {
      onOpenProject(job.projectId);
      message.info(result.currentNodeId === "export_publish"
        ? "项目已准备好，请在编辑器中预览并导出"
        : "工作流正在等待你的确认，已打开对应项目");
    }
    return result;
  }, [onOpenProject, queryClient]);
}
