import { useState, type FormEvent } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { Agent, CreateAgentInput } from "@/lib/api/agents";

export type AgentFormResult = "success" | "conflict" | "error";

interface AgentFormDialogProps {
  agent?: Agent;
  pending: boolean;
  onClose: () => void;
  onSubmit: (
    input: CreateAgentInput,
    expectedRevision?: number,
  ) => Promise<AgentFormResult>;
  onReloadLatest: (id: string) => Promise<Agent | undefined>;
}

export function AgentFormDialog({
  agent,
  pending,
  onClose,
  onSubmit,
  onReloadLatest,
}: AgentFormDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(agent?.name ?? "");
  const [owner, setOwner] = useState(agent?.owner ?? "");
  const [description, setDescription] = useState(agent?.description ?? "");
  const [expectedRevision, setExpectedRevision] = useState(
    agent?.revision ?? 0,
  );
  const [conflict, setConflict] = useState(false);
  const [reloading, setReloading] = useState(false);

  const isEditing = Boolean(agent);
  const valid = name.trim().length > 0 && owner.trim().length > 0;

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!valid || pending) return;

    const result = await onSubmit(
      {
        name: name.trim(),
        owner: owner.trim(),
        description: description.trim(),
      },
      isEditing ? expectedRevision : undefined,
    );
    setConflict(result === "conflict");
  };

  const reloadLatest = async () => {
    if (!agent) return;
    setReloading(true);
    try {
      const latest = await onReloadLatest(agent.id);
      if (!latest) return;
      setName(latest.name);
      setOwner(latest.owner);
      setDescription(latest.description);
      setExpectedRevision(latest.revision);
      setConflict(false);
    } finally {
      setReloading(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) onClose();
      }}
    >
      <DialogContent className="max-w-lg">
        <form onSubmit={handleSubmit}>
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle>
              {t(isEditing ? "agents.editTitle" : "agents.createTitle")}
            </DialogTitle>
            <DialogDescription>
              {t(
                isEditing
                  ? "agents.editDescription"
                  : "agents.createDescription",
              )}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 px-6 py-4">
            {conflict ? (
              <div
                role="alert"
                className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3"
              >
                <div className="flex items-start gap-2.5">
                  <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium">
                      {t("agents.conflictTitle")}
                    </p>
                    <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                      {t("agents.conflictDescription")}
                    </p>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="mt-3"
                      disabled={reloading}
                      onClick={reloadLatest}
                    >
                      <RefreshCw
                        className={`mr-2 h-3.5 w-3.5 ${reloading ? "animate-spin" : ""}`}
                      />
                      {t("agents.reloadLatest")}
                    </Button>
                  </div>
                </div>
              </div>
            ) : null}

            <div className="space-y-2">
              <Label htmlFor="agent-name">{t("agents.name")}</Label>
              <Input
                id="agent-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t("agents.namePlaceholder")}
                autoFocus
                disabled={pending}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="agent-owner">{t("agents.owner")}</Label>
              <Input
                id="agent-owner"
                value={owner}
                onChange={(event) => setOwner(event.target.value)}
                placeholder={t("agents.ownerPlaceholder")}
                disabled={pending}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="agent-description">
                {t("agents.description")}
              </Label>
              <Textarea
                id="agent-description"
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                placeholder={t("agents.descriptionPlaceholder")}
                className="min-h-24 resize-none"
                disabled={pending}
              />
            </div>
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={pending}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!valid || pending}>
              {t(isEditing ? "agents.save" : "agents.create")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
