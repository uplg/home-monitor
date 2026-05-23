import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, WifiOff } from "lucide-react";
import { broadlinkApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { toast } from "@/hooks/use-toast";

interface BroadlinkClimateControlProps {
  defaultModel?: string;
  compact?: boolean;
  showRefresh?: boolean;
}

type ClimateMode = "cool" | "heat" | "dry" | "fan" | "auto";
type ClimateFan = "auto" | "1" | "2" | "3" | "4" | "silent";
type ClimateVane = "auto" | "highest" | "high" | "middle" | "low" | "lowest" | "swing";
type ClimateTimerMode = "none" | "stop";

interface StructuredState {
  power: boolean;
  mode: ClimateMode;
  temperature: number;
  fan: ClimateFan;
  vane: ClimateVane;
  econo: boolean;
  timerMode: ClimateTimerMode;
  stopHour: string;
  stopMinute: string;
}

const DISCOVERY_TIMEOUT_MS = 120_000;
const INITIAL_STATE: StructuredState = {
  power: true,
  mode: "cool",
  temperature: 20,
  fan: "auto",
  vane: "auto",
  econo: false,
  timerMode: "none",
  stopHour: "11",
  stopMinute: "00",
};

export function BroadlinkClimateControl({
  defaultModel = "msz-hj5va",
  compact = false,
  showRefresh = true,
}: BroadlinkClimateControlProps) {
  const { t } = useTranslation();
  const [discoveryTimedOut, setDiscoveryTimedOut] = useState(false);
  const [forceRefreshToken, setForceRefreshToken] = useState(0);
  const [structuredState, setStructuredState] = useState<StructuredState>(INITIAL_STATE);
  // Free-text buffer for the temperature field so the user can type anything
  // (partial values, empty); we only clamp/validate on blur. Kept in sync with
  // the committed temperature (reset button, external updates).
  const [tempInput, setTempInput] = useState(String(INITIAL_STATE.temperature));

  useEffect(() => {
    setTempInput(String(structuredState.temperature));
  }, [structuredState.temperature]);

  const discoverQuery = useQuery({
    queryKey: ["broadlink", "discover", "single-remote", forceRefreshToken],
    queryFn: () => broadlinkApi.discover(undefined, forceRefreshToken > 0),
    retry: true,
    retryDelay: 4000,
    refetchInterval: (query) => {
      const hasRemote = (query.state.data?.devices?.length ?? 0) > 0;
      return hasRemote || discoveryTimedOut ? false : 4000;
    },
    refetchOnWindowFocus: false,
    staleTime: Infinity,
  });

  const remote = discoverQuery.data?.devices?.[0];
  const structuredCommand = useMemo(
    () => buildStructuredCommand(structuredState),
    [structuredState],
  );

  useEffect(() => {
    if (remote || discoveryTimedOut) {
      return;
    }

    const timeout = window.setTimeout(() => {
      setDiscoveryTimedOut(true);
    }, DISCOVERY_TIMEOUT_MS);

    return () => window.clearTimeout(timeout);
  }, [remote, discoveryTimedOut]);

  useEffect(() => {
    if (remote?.host) {
      setDiscoveryTimedOut(false);
    }
  }, [remote]);

  const sendMutation = useMutation({
    mutationFn: (command: string) =>
      broadlinkApi.sendMitsubishiCommand(remote?.host ?? "", command, defaultModel),
    onSuccess: (_, command) => {
      toast({
        title: t("climate.commandSent"),
        description: t("climate.commandSentDescription", {
          command,
          host: remote?.host ?? t("climate.remoteFallbackHost"),
        }),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("climate.commandFailed"),
        variant: "destructive",
      });
    },
  });

  return (
    <Card className="border-0 bg-transparent shadow-none">
      {showRefresh && (
        <CardHeader className={compact ? "pb-3" : "pb-4"}>
          <div className="flex items-center justify-end gap-4">
            <Button
              variant="ghost"
              size="icon"
              className="rounded-full text-muted-foreground hover:bg-accent hover:text-accent-foreground"
              onClick={() => {
                setDiscoveryTimedOut(false);
                setForceRefreshToken((value) => value + 1);
                discoverQuery.refetch();
              }}
              disabled={discoverQuery.isFetching || sendMutation.isPending}
            >
              {discoverQuery.isFetching ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="h-4 w-4" />
              )}
            </Button>
          </div>
        </CardHeader>
      )}

      <CardContent className="space-y-4 px-0 pb-0">
        {remote ? (
          <div className="rounded-2xl bg-emerald-50/90 px-4 py-3 text-sm text-emerald-800">
            <div className="font-medium">{t("climate.remoteConnected")}</div>
            <div className="mt-1 font-mono text-xs text-emerald-700">{remote.host}</div>
          </div>
        ) : discoveryTimedOut ? (
          <div className="rounded-2xl bg-muted px-4 py-5 text-center text-sm text-muted-foreground">
            <WifiOff className="mx-auto mb-3 h-5 w-5 text-muted-foreground" />
            <div className="font-medium text-muted-foreground">{t("climate.noRemoteTitle")}</div>
            <div className="mt-1">{t("climate.noRemoteDescription")}</div>
          </div>
        ) : (
          <div className="rounded-2xl bg-muted px-4 py-5">
            <div className="mb-4 flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-2xl bg-sky-100 text-sky-600">
                <Loader2 className="h-4 w-4 animate-spin" />
              </div>
              <div>
                <div className="font-medium text-foreground">{t("climate.searchingTitle")}</div>
                <div className="text-sm text-muted-foreground">
                  {t("climate.searchingDescription")}
                </div>
              </div>
            </div>
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <Skeleton className="h-14 rounded-2xl bg-card" />
              <Skeleton className="h-14 rounded-2xl bg-card" />
              <Skeleton className="h-14 rounded-2xl bg-card" />
            </div>
          </div>
        )}

        {remote && (
          <div className="space-y-4">
            <div className="rounded-3xl border border-border bg-card p-4 shadow-sm">
              <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div>
                  <div className="text-sm font-semibold text-foreground">
                    {t("climate.brandName")}
                  </div>
                  <div className="mt-1 text-sm text-muted-foreground">
                    {t("climate.structuredDescription")}
                  </div>
                </div>
                <Button
                  variant="outline"
                  className="rounded-2xl"
                  onClick={() => setStructuredState(INITIAL_STATE)}
                  disabled={sendMutation.isPending}
                >
                  {t("climate.reset")}
                </Button>
              </div>

              <div className="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                <ControlBlock label={t("climate.power")}>
                  <div className="flex gap-2">
                    <Button
                      type="button"
                      variant={structuredState.power ? "default" : "outline"}
                      className="flex-1 rounded-2xl"
                      onClick={() => setStructuredState((current) => ({ ...current, power: true }))}
                      disabled={sendMutation.isPending}
                    >
                      {t("climate.on")}
                    </Button>
                    <Button
                      type="button"
                      variant={!structuredState.power ? "default" : "outline"}
                      className="flex-1 rounded-2xl"
                      onClick={() =>
                        setStructuredState((current) => ({ ...current, power: false }))
                      }
                      disabled={sendMutation.isPending}
                    >
                      {t("climate.off")}
                    </Button>
                  </div>
                </ControlBlock>

                <ControlBlock label={t("climate.mode")}>
                  <Select
                    value={structuredState.mode}
                    onValueChange={(value: ClimateMode) =>
                      setStructuredState((current) => ({
                        ...current,
                        mode: value,
                        econo: value === "cool" ? current.econo : false,
                      }))
                    }
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="cool">{t("climate.modes.cool")}</SelectItem>
                      <SelectItem value="heat">{t("climate.modes.heat")}</SelectItem>
                      <SelectItem value="dry">{t("climate.modes.dry")}</SelectItem>
                      <SelectItem value="fan">{t("climate.modes.fan")}</SelectItem>
                      <SelectItem value="auto">{t("climate.modes.auto")}</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label={t("climate.temperature")}>
                  <Input
                    type="number"
                    inputMode="numeric"
                    min={16}
                    max={31}
                    value={tempInput}
                    className="rounded-2xl"
                    onChange={(event) => setTempInput(event.target.value)}
                    onBlur={() => {
                      const parsed = Number(tempInput);
                      if (tempInput.trim() === "" || !Number.isFinite(parsed)) {
                        // Invalid/empty entry: revert to the last committed value.
                        setTempInput(String(structuredState.temperature));
                        return;
                      }
                      const clamped = Math.min(31, Math.max(16, Math.round(parsed)));
                      setStructuredState((current) => ({ ...current, temperature: clamped }));
                      setTempInput(String(clamped));
                    }}
                  />
                </ControlBlock>

                <ControlBlock label={t("climate.fan")}>
                  <Select
                    value={structuredState.fan}
                    onValueChange={(value: ClimateFan) =>
                      setStructuredState((current) => ({ ...current, fan: value }))
                    }
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">{t("climate.fanLevels.auto")}</SelectItem>
                      <SelectItem value="1">
                        {t("climate.fanLevels.level", { level: 1 })}
                      </SelectItem>
                      <SelectItem value="2">
                        {t("climate.fanLevels.level", { level: 2 })}
                      </SelectItem>
                      <SelectItem value="3">
                        {t("climate.fanLevels.level", { level: 3 })}
                      </SelectItem>
                      <SelectItem value="4">
                        {t("climate.fanLevels.level", { level: 4 })}
                      </SelectItem>
                      <SelectItem value="silent">{t("climate.fanLevels.silent")}</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label={t("climate.verticalVane")}>
                  <Select
                    value={structuredState.vane}
                    onValueChange={(value: ClimateVane) =>
                      setStructuredState((current) => ({ ...current, vane: value }))
                    }
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">{t("climate.vanes.auto")}</SelectItem>
                      <SelectItem value="highest">{t("climate.vanes.highest")}</SelectItem>
                      <SelectItem value="high">{t("climate.vanes.high")}</SelectItem>
                      <SelectItem value="middle">{t("climate.vanes.middle")}</SelectItem>
                      <SelectItem value="low">{t("climate.vanes.low")}</SelectItem>
                      <SelectItem value="lowest">{t("climate.vanes.lowest")}</SelectItem>
                      <SelectItem value="swing">{t("climate.vanes.swing")}</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label={t("climate.timerMode")}>
                  <Select
                    value={structuredState.timerMode}
                    onValueChange={(value: ClimateTimerMode) =>
                      setStructuredState((current) => ({ ...current, timerMode: value }))
                    }
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">{t("climate.timerModes.none")}</SelectItem>
                      <SelectItem value="stop">{t("climate.timerModes.stop")}</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label={t("climate.stopTime")}>
                  <div className="grid grid-cols-2 gap-2">
                    <Input
                      value={structuredState.stopHour}
                      className="rounded-2xl"
                      onChange={(event) =>
                        setStructuredState((current) => ({
                          ...current,
                          stopHour: sanitizeTimePart(event.target.value, 23),
                        }))
                      }
                    />
                    <Input
                      value={structuredState.stopMinute}
                      className="rounded-2xl"
                      onChange={(event) =>
                        setStructuredState((current) => ({
                          ...current,
                          stopMinute: sanitizeMinutePart(event.target.value),
                        }))
                      }
                    />
                  </div>
                </ControlBlock>
              </div>

              <div className="mt-4 grid gap-3 md:grid-cols-2">
                <ToggleCard
                  title={t("climate.econoCool")}
                  description={t("climate.econoCoolDescription")}
                  checked={structuredState.econo}
                  disabled={structuredState.mode !== "cool"}
                  onCheckedChange={(checked) =>
                    setStructuredState((current) => ({ ...current, econo: checked }))
                  }
                />
              </div>

              <div className="mt-4 rounded-2xl bg-muted px-4 py-3">
                <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {t("climate.generatedCommand")}
                </div>
                <div className="mt-1 break-all font-mono text-sm text-foreground">
                  {structuredCommand}
                </div>
              </div>

              <div className="mt-4 flex flex-wrap gap-3">
                <Button
                  className="rounded-2xl"
                  disabled={sendMutation.isPending}
                  onClick={() => sendMutation.mutate(structuredCommand)}
                >
                  {sendMutation.isPending && sendMutation.variables === structuredCommand ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : null}
                  {t("climate.sendStructuredCommand")}
                </Button>
                <Button
                  variant="outline"
                  className="rounded-2xl"
                  disabled={sendMutation.isPending}
                  onClick={() => sendMutation.mutate("state-off")}
                >
                  {t("climate.sendOff")}
                </Button>
              </div>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function ControlBlock({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-2">
      <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      {children}
    </div>
  );
}

function ToggleCard({
  title,
  description,
  checked,
  disabled,
  onCheckedChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between rounded-2xl border border-border px-4 py-3">
      <div>
        <div className="text-sm font-medium text-foreground">{title}</div>
        <div className="text-xs text-muted-foreground">{description}</div>
      </div>
      <Switch checked={checked} disabled={disabled} onCheckedChange={onCheckedChange} />
    </div>
  );
}

function buildStructuredCommand(state: StructuredState) {
  if (!state.power) {
    return "state-off";
  }

  const parts = [
    "state",
    state.mode,
    String(state.temperature),
    "fan",
    state.fan,
    "vane",
    state.vane,
    "wide",
    "center",
  ];

  if (state.econo) {
    parts.push("econo", "on");
  }

  if (state.timerMode === "stop") {
    parts.push("stop", state.stopHour.padStart(2, "0"), state.stopMinute.padStart(2, "0"));
  }

  return parts.join("-");
}

function sanitizeTimePart(value: string, max: number) {
  const digits = value.replace(/\D/g, "").slice(0, 2);
  if (!digits) return "";
  return String(Math.min(max, Number(digits))).padStart(2, "0");
}

function sanitizeMinutePart(value: string) {
  const digits = value.replace(/\D/g, "").slice(0, 2);
  if (!digits) return "";
  const numeric = Math.min(59, Number(digits));
  return String(numeric - (numeric % 10)).padStart(2, "0");
}
