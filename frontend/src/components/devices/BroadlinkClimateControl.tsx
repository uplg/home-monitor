import type { ReactNode } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, WifiOff } from "lucide-react";
import { broadlinkApi } from "@/lib/api";
import type { BroadlinkClimateSettings } from "@/lib/api";
import { formatMinutes } from "@/lib/format";
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

// Single source for the accepted token lists: the union types, the runtime
// guards, and (via the guards) restored-state validation all derive from
// these arrays.
const CLIMATE_MODES = ["cool", "heat", "dry", "fan", "auto"] as const;
const CLIMATE_FANS = ["auto", "1", "2", "3", "4", "silent"] as const;
const CLIMATE_VANES = ["auto", "highest", "high", "middle", "low", "lowest", "swing"] as const;

type ClimateMode = (typeof CLIMATE_MODES)[number];
type ClimateFan = (typeof CLIMATE_FANS)[number];
type ClimateVane = (typeof CLIMATE_VANES)[number];
type ClimateTimerMode = "none" | "stop";

/** Temperature setpoint bounds supported by the Mitsubishi protocol (°C). */
const TEMP_MIN_C = 16;
const TEMP_MAX_C = 31;

interface StructuredState {
  power: boolean;
  mode: ClimateMode;
  temperature: number;
  fan: ClimateFan;
  vane: ClimateVane;
  econo: boolean;
  timerMode: ClimateTimerMode;
  /** Sleep-timer delay ("turn off in N minutes"), like the remote's 1h/3h buttons. */
  stopAfterMinutes: number;
}

const DISCOVERY_TIMEOUT_MS = 120_000;
// Multiples of 10 minutes only: the Mitsubishi timer works in 10-minute ticks.
const STOP_AFTER_CHOICES = [30, 60, 120, 180, 300, 480, 720] as const;
const INITIAL_STATE: StructuredState = {
  power: true,
  mode: "cool",
  temperature: 20,
  fan: "auto",
  vane: "auto",
  econo: false,
  timerMode: "none",
  stopAfterMinutes: 180,
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

  // Restore the last commanded state (persisted server-side) once on mount.
  // One-shot by design: re-applying it later would clobber in-progress edits.
  const hydratedRef = useRef(false);
  const climateStateQuery = useQuery({
    queryKey: ["broadlink", "mitsubishi", "state"],
    queryFn: () => broadlinkApi.getMitsubishiState(),
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });
  useEffect(() => {
    const stored = climateStateQuery.data?.state;
    if (hydratedRef.current || !stored) return;
    hydratedRef.current = true;
    if (stored.settings) {
      setStructuredState(stateFromSettings(stored.settings, stored.power));
    } else if (!stored.power) {
      setStructuredState((current) => ({ ...current, power: false }));
    }
  }, [climateStateQuery.data]);

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
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1 px-1 text-sm">
            <span className="h-2 w-2 shrink-0 rounded-full bg-emerald-500" aria-hidden />
            <span className="font-medium text-foreground">{t("climate.remoteConnected")}</span>
            <span className="font-mono text-xs text-muted-foreground">{remote.host}</span>
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
              <div className="flex h-10 w-10 items-center justify-center rounded-2xl bg-sky-100 text-sky-600 dark:bg-sky-950/50 dark:text-sky-300">
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
                    min={TEMP_MIN_C}
                    max={TEMP_MAX_C}
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
                      const clamped = Math.min(
                        TEMP_MAX_C,
                        Math.max(TEMP_MIN_C, Math.round(parsed)),
                      );
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

                {structuredState.timerMode === "stop" && (
                  <ControlBlock label={t("climate.stopAfter")}>
                    <Select
                      value={String(structuredState.stopAfterMinutes)}
                      onValueChange={(value) =>
                        setStructuredState((current) => ({
                          ...current,
                          stopAfterMinutes: Number(value),
                        }))
                      }
                    >
                      <SelectTrigger className="rounded-2xl">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {/* Include the current value even when it isn't a
                            preset (a restored state may carry any multiple of
                            10 minutes), so the control never renders blank. */}
                        {stopAfterOptions(structuredState.stopAfterMinutes).map((minutes) => (
                          <SelectItem key={minutes} value={String(minutes)}>
                            {formatMinutes(minutes)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </ControlBlock>
                )}
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
    parts.push("stopin", String(state.stopAfterMinutes));
  }

  return parts.join("-");
}

/**
 * Builds form state from the backend-parsed settings of the last command.
 * Individual unknown values fall back to the defaults rather than rejecting
 * the whole restored state.
 */
function stateFromSettings(settings: BroadlinkClimateSettings, power: boolean): StructuredState {
  return {
    power,
    mode: isClimateMode(settings.mode) ? settings.mode : INITIAL_STATE.mode,
    temperature: Math.min(TEMP_MAX_C, Math.max(TEMP_MIN_C, Math.round(settings.temperature))),
    fan: isClimateFan(settings.fan) ? settings.fan : INITIAL_STATE.fan,
    vane: isClimateVane(settings.vane) ? settings.vane : INITIAL_STATE.vane,
    econo: settings.econo,
    timerMode: settings.stopInMinutes ? "stop" : "none",
    stopAfterMinutes: settings.stopInMinutes ?? INITIAL_STATE.stopAfterMinutes,
  };
}

function isClimateMode(value: string): value is ClimateMode {
  return (CLIMATE_MODES as readonly string[]).includes(value);
}

function isClimateFan(value: string): value is ClimateFan {
  return (CLIMATE_FANS as readonly string[]).includes(value);
}

function isClimateVane(value: string): value is ClimateVane {
  return (CLIMATE_VANES as readonly string[]).includes(value);
}

function stopAfterOptions(currentMinutes: number): number[] {
  const options = new Set<number>(STOP_AFTER_CHOICES);
  if (Number.isInteger(currentMinutes) && currentMinutes > 0) {
    options.add(currentMinutes);
  }
  return [...options].sort((left, right) => left - right);
}
