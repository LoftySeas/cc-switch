import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { agentOsApi } from "./agentOs";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("agentOsApi", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("uses product commands without sending provider, model, or runtime decisions", async () => {
    vi.mocked(invoke).mockResolvedValue([]);

    await agentOsApi.listWorkflows();
    await agentOsApi.listWorkflowRuns("workflow:one");
    await agentOsApi.cancelWorkflowRun("run:one", 4);
    await agentOsApi.listExecutions();

    expect(invoke).toHaveBeenNthCalledWith(1, "list_agent_os_workflows");
    expect(invoke).toHaveBeenNthCalledWith(2, "list_agent_os_workflow_runs", {
      workflowId: "workflow:one",
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "cancel_agent_os_workflow_run", {
      runId: "run:one",
      expectedRevision: 4,
    });
    expect(invoke).toHaveBeenNthCalledWith(4, "list_agent_os_executions");
  });
});
