import { useEffect, useState } from "react";
import { Copy, KeyRound, RefreshCw, TerminalSquare } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { settingsApi, type AgentApiInfo } from "@/lib/api/settings";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

const DEFAULT_PORT = 15722;

export function AgentApiSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [status, setStatus] = useState<AgentApiInfo | null>(null);
  const [port, setPort] = useState(DEFAULT_PORT);
  const [revealedToken, setRevealedToken] = useState<string>();
  const [pending, setPending] = useState(false);

  useEffect(() => {
    void settingsApi
      .getAgentApiStatus()
      .then((next) => {
        setStatus(next);
        setPort(next.port);
      })
      .catch((error) => console.warn("Failed to read Agent API status", error));
  }, []);

  const configure = async (enabled: boolean) => {
    setPending(true);
    try {
      const next = await settingsApi.configureAgentApi(enabled, port);
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      setStatus(next);
      if (next.token) setRevealedToken(next.token);
      toast.success(
        enabled
          ? t("settings.agentApi.started")
          : t("settings.agentApi.stopped"),
      );
    } catch (error) {
      toast.error(String(error));
    } finally {
      setPending(false);
    }
  };

  const rotate = async () => {
    setPending(true);
    try {
      const next = await settingsApi.rotateAgentApiToken();
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      setStatus(next);
      setRevealedToken(next.token);
      toast.success(t("settings.agentApi.rotated"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setPending(false);
    }
  };

  const copy = async (value: string) => {
    await navigator.clipboard.writeText(value);
    toast.success(t("settings.agentApi.copied"));
  };

  const curl = status
    ? `curl -s -H "Authorization: Bearer $CCSWITCH_AGENT_API_TOKEN" "${status.url}/_ccswitch/v1/usage?app=claude&provider=active"`
    : "";

  return (
    <div className="space-y-5">
      <div className="flex items-center justify-between gap-4">
        <div>
          <Label className="text-sm font-medium">
            {t("settings.agentApi.enable")}
          </Label>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("settings.agentApi.securityHint")}
          </p>
        </div>
        <Switch
          checked={status?.enabled ?? false}
          disabled={pending}
          onCheckedChange={(checked) => void configure(checked)}
        />
      </div>

      <div className="grid gap-2 sm:grid-cols-[180px_1fr] sm:items-end">
        <div className="space-y-2">
          <Label htmlFor="agent-api-port">{t("settings.agentApi.port")}</Label>
          <Input
            id="agent-api-port"
            type="number"
            min={1}
            max={65535}
            value={port}
            disabled={pending}
            onChange={(event) => setPort(Number(event.target.value))}
          />
        </div>
        <Button
          variant="outline"
          disabled={pending || !status?.enabled || port === status.port}
          onClick={() => void configure(true)}
        >
          <RefreshCw className="mr-2 h-4 w-4" />
          {t("settings.agentApi.applyPort")}
        </Button>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          disabled={pending}
          onClick={() => void rotate()}
        >
          <KeyRound className="mr-2 h-4 w-4" />
          {t("settings.agentApi.rotateToken")}
        </Button>
        {status?.tokenConfigured && !revealedToken && (
          <span className="text-xs text-muted-foreground">
            {t("settings.agentApi.tokenHidden")}
          </span>
        )}
      </div>

      {revealedToken && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3">
          <p className="mb-2 text-xs text-amber-700 dark:text-amber-300">
            {t("settings.agentApi.tokenOnce")}
          </p>
          <div className="flex gap-2">
            <Input
              value={revealedToken}
              readOnly
              className="font-mono text-xs"
            />
            <Button
              variant="outline"
              size="icon"
              onClick={() => void copy(revealedToken)}
            >
              <Copy className="h-4 w-4" />
            </Button>
          </div>
        </div>
      )}

      {status?.running && (
        <div className="rounded-lg bg-muted/60 p-3">
          <div className="mb-2 flex items-center gap-2 text-xs font-medium">
            <TerminalSquare className="h-4 w-4" />
            {t("settings.agentApi.example")}
          </div>
          <div className="flex gap-2">
            <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap text-xs">
              {curl}
            </code>
            <Button variant="ghost" size="icon" onClick={() => void copy(curl)}>
              <Copy className="h-4 w-4" />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
