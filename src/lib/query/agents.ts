import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  agentsApi,
  type Agent,
  type CreateAgentInput,
  type UpdateAgentInput,
} from "@/lib/api/agents";
import { extractErrorMessage } from "@/utils/errorUtils";

export const agentKeys = {
  all: ["agents"] as const,
};

export const isAgentRevisionConflict = (error: unknown): boolean => {
  const message = extractErrorMessage(error).toLowerCase();
  return (
    message.includes("revision conflict") ||
    message.includes("changed concurrently")
  );
};

const replaceAgent = (agents: Agent[] | undefined, updated: Agent): Agent[] => {
  if (!agents) return [updated];
  return agents.map((agent) => (agent.id === updated.id ? updated : agent));
};

export const useAgentsQuery = () =>
  useQuery({
    queryKey: agentKeys.all,
    queryFn: agentsApi.list,
  });

export const useCreateAgentMutation = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: CreateAgentInput) => agentsApi.create(input),
    onSuccess: (created) => {
      queryClient.setQueryData<Agent[]>(agentKeys.all, (agents) => [
        ...(agents ?? []),
        created,
      ]);
    },
  });
};

export const useUpdateAgentMutation = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: UpdateAgentInput }) =>
      agentsApi.update(id, input),
    onSuccess: (updated) => {
      queryClient.setQueryData<Agent[]>(agentKeys.all, (agents) =>
        replaceAgent(agents, updated),
      );
    },
  });
};

export const useRetireAgentMutation = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, revision }: { id: string; revision: number }) =>
      agentsApi.setLifecycle(id, "retired", revision),
    onSuccess: (updated) => {
      queryClient.setQueryData<Agent[]>(agentKeys.all, (agents) =>
        replaceAgent(agents, updated),
      );
    },
  });
};
