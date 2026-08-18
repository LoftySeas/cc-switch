import { invoke } from "@tauri-apps/api/core";

export type WorkflowRunLifecycle =
  | "draft"
  | "ready"
  | "running"
  | "waiting"
  | "succeeded"
  | "failed"
  | "cancelled";

export interface WorkflowStepDefinition {
  id: string;
  name: string;
  objective: string;
  roleId: string;
  roleVersion: number;
  dependencies: string[];
  acceptanceCriteria: string[];
}

export interface WorkflowDefinition {
  id: string;
  version: number;
  teamId: string;
  name: string;
  purpose: string;
  steps: WorkflowStepDefinition[];
  createdAt: number;
}

export interface WorkflowRun {
  id: string;
  workflowId: string;
  workflowVersion: number;
  teamId: string;
  lifecycle: WorkflowRunLifecycle;
  stepStates: Record<string, string>;
  revision: number;
  createdAt: number;
  updatedAt: number;
}

export interface WorkflowTask {
  id: string;
  runId: string;
  stepId: string;
  agentId: string;
  executionId: string;
  lifecycle: string;
  revision: number;
}

export interface ExecutionManagementView {
  executionId: string;
  objective: string;
  state: string;
  revision: number;
  transitionCount: number;
  agentId: string;
  runtimeId: string;
  modelId: string;
  contextReferences: string[];
  resultSummary?: string;
  acceptedAt: number;
}

export interface ExecutionRecord {
  request: {
    context: {
      executionId: string;
      binding: {
        id: string;
        agentId: string;
        runtimeId: string;
      };
      contextReferences: string[];
    };
    objective: string;
    modelBinding: {
      modelId: string;
      providerId?: string;
      modelAvailabilityId?: string;
    };
    correlationRef?: string;
    acceptedAt: number;
  };
  state: string;
  revision: number;
  transitions: Array<{
    sequence: number;
    from: string;
    to: string;
    occurredAt: number;
    reason: string;
  }>;
  result?: {
    summary: string;
    artifactReferences: string[];
    completedAt: number;
  };
}

export const agentOsApi = {
  listWorkflows: (): Promise<WorkflowDefinition[]> =>
    invoke("list_agent_os_workflows"),
  listWorkflowRuns: (workflowId: string): Promise<WorkflowRun[]> =>
    invoke("list_agent_os_workflow_runs", { workflowId }),
  listWorkflowTasks: (runId: string): Promise<WorkflowTask[]> =>
    invoke("list_agent_os_workflow_tasks", { runId }),
  cancelWorkflowRun: (
    runId: string,
    expectedRevision: number,
  ): Promise<WorkflowRun> =>
    invoke("cancel_agent_os_workflow_run", { runId, expectedRevision }),
  listExecutions: (): Promise<ExecutionRecord[]> =>
    invoke("list_agent_os_executions"),
  getExecution: (executionId: string): Promise<ExecutionRecord | null> =>
    invoke("get_agent_os_execution", { executionId }),
  listExecutionViews: (): Promise<ExecutionManagementView[]> =>
    invoke("list_agent_os_execution_views"),
  getExecutionView: (
    executionId: string,
  ): Promise<ExecutionManagementView | null> =>
    invoke("get_agent_os_execution_view", { executionId }),
};
