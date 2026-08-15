import { invoke } from "@tauri-apps/api/core";

export type AgentLifecycle = "draft" | "active" | "suspended" | "retired";

export interface Agent {
  id: string;
  name: string;
  description: string;
  owner: string;
  lifecycleState: AgentLifecycle;
  revision: number;
  createdAt: number;
  updatedAt: number;
}

export interface CreateAgentInput {
  name: string;
  description?: string;
  owner: string;
}

export interface UpdateAgentInput {
  expectedRevision: number;
  name?: string;
  description?: string;
  owner?: string;
}

export const agentsApi = {
  async list(): Promise<Agent[]> {
    return await invoke("list_agents");
  },

  async get(id: string): Promise<Agent> {
    return await invoke("get_agent", { id });
  },

  async create(input: CreateAgentInput): Promise<Agent> {
    return await invoke("create_agent", { input });
  },

  async update(id: string, input: UpdateAgentInput): Promise<Agent> {
    return await invoke("update_agent", { id, input });
  },

  async setLifecycle(
    id: string,
    lifecycleState: AgentLifecycle,
    expectedRevision: number,
  ): Promise<Agent> {
    return await invoke("set_agent_lifecycle", {
      id,
      lifecycleState,
      expectedRevision,
    });
  },
};
