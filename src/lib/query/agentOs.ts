import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  agentOsApi,
  type WorkflowDefinition,
  type WorkflowRun,
} from "@/lib/api/agentOs";

export interface WorkflowOverview {
  definition: WorkflowDefinition;
  runs: WorkflowRun[];
}

export const agentOsKeys = {
  workflows: ["agent-os", "workflows"] as const,
  executions: ["agent-os", "executions"] as const,
};

export const useWorkflowOverviewsQuery = () =>
  useQuery({
    queryKey: agentOsKeys.workflows,
    queryFn: async (): Promise<WorkflowOverview[]> => {
      const definitions = await agentOsApi.listWorkflows();
      return await Promise.all(
        definitions.map(async (definition) => ({
          definition,
          runs: await agentOsApi.listWorkflowRuns(definition.id),
        })),
      );
    },
  });

export const useCancelWorkflowRunMutation = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      runId,
      expectedRevision,
    }: {
      runId: string;
      expectedRevision: number;
    }) => agentOsApi.cancelWorkflowRun(runId, expectedRevision),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: agentOsKeys.workflows }),
  });
};

export const useExecutionsQuery = () =>
  useQuery({
    queryKey: agentOsKeys.executions,
    queryFn: agentOsApi.listExecutionViews,
  });
