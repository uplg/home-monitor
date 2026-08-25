import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Home,
  Loader2,
  Power,
  Settings2,
  Sparkles,
  Tv,
  Volume2,
  VolumeX,
} from "lucide-react";
import { tvApi, type TvConfig, type TvKey, type TvStatus } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { toast } from "@/hooks/use-toast";

/**
 * TV shelf: power, volume, Ambilight and a small D-pad, driven by JointSPACE.
 *
 * Polling is deliberately slow. The set's HTTP server is single-threaded and
 * dies for good under bursts (only a mains cycle revives it), so the dashboard
 * asks rarely and the backend spaces every call it makes.
 */
export function TvControl() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showConfig, setShowConfig] = useState(false);
  const [draft, setDraft] = useState<TvConfig>({});
  const [volumeDraft, setVolumeDraft] = useState<number | null>(null);

  const statusQuery = useQuery({
    queryKey: ["tv-status"],
    queryFn: tvApi.status,
    staleTime: 15_000,
    refetchInterval: 60_000,
  });

  const config = statusQuery.data?.config;
  const status: TvStatus | undefined = statusQuery.data?.status;

  useEffect(() => {
    if (config) setDraft(config);
  }, [config]);

  // Let the server's reading win again once the local drag has been applied.
  useEffect(() => {
    setVolumeDraft(null);
  }, [status?.volume?.current]);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["tv-status"] });

  const fail = (error: unknown) =>
    toast({
      title: t("common.error"),
      description: error instanceof Error ? error.message : String(error),
      variant: "destructive",
    });

  const powerMutation = useMutation({
    mutationFn: (state: "on" | "off" | "toggle") => tvApi.power(state),
    onSuccess: invalidate,
    onError: fail,
  });

  const keyMutation = useMutation({
    mutationFn: (key: TvKey) => tvApi.sendKey(key),
    onSuccess: invalidate,
    onError: fail,
  });

  const volumeMutation = useMutation({
    mutationFn: ({ level, muted }: { level?: number; muted?: boolean }) =>
      tvApi.setVolume(level, muted),
    onSuccess: invalidate,
    onError: fail,
  });

  const ambilightMutation = useMutation({
    mutationFn: () => tvApi.ambilight("toggle"),
    onSuccess: invalidate,
    onError: fail,
  });

  const boxMutation = useMutation({
    mutationFn: () => tvApi.switchToBox(),
    onSuccess: (response) => {
      invalidate();
      toast({ title: t("tv.switchToBox"), description: response.message });
    },
    onError: fail,
  });

  const configMutation = useMutation({
    mutationFn: (next: TvConfig) => tvApi.setConfig(next),
    onSuccess: () => {
      invalidate();
      setShowConfig(false);
      toast({ title: t("tv.saved") });
    },
    onError: fail,
  });

  const isOn = status?.power === "on";
  const isDeep = status?.power === "deep_standby";
  const volume = status?.volume;
  const shownVolume = volumeDraft ?? volume?.current ?? 0;

  const powerLabel = isOn ? t("tv.on") : isDeep ? t("tv.deepStandby") : t("tv.standby");

  const dpad: Array<{ key: TvKey; icon: typeof ChevronUp; className: string }> = [
    { key: "cursor_up", icon: ChevronUp, className: "col-start-2 row-start-1" },
    { key: "cursor_left", icon: ChevronLeft, className: "col-start-1 row-start-2" },
    { key: "cursor_right", icon: ChevronRight, className: "col-start-3 row-start-2" },
    { key: "cursor_down", icon: ChevronDown, className: "col-start-2 row-start-3" },
  ];

  return (
    <Card className="border-border/60 bg-card/60">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-3">
        <div className="flex items-center gap-3 min-w-0">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-muted text-muted-foreground">
            <Tv className="h-5 w-5" />
          </div>
          <div className="min-w-0">
            <CardTitle className="text-[1.1rem] tracking-[-0.02em]">
              {status?.name || t("tv.title")}
            </CardTitle>
            <p className="truncate text-sm text-muted-foreground">{t("tv.subtitle")}</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {status?.configured ? (
            <Badge variant={isOn ? "default" : "secondary"}>{powerLabel}</Badge>
          ) : null}
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("tv.configure")}
            onClick={() => setShowConfig((open) => !open)}
          >
            <Settings2 className="h-4 w-4" />
          </Button>
        </div>
      </CardHeader>

      <CardContent className="space-y-4">
        {showConfig || !status?.configured ? (
          <div className="space-y-3 rounded-2xl border border-border/60 bg-background/40 p-3">
            {!status?.configured ? (
              <p className="text-sm text-muted-foreground">{t("tv.configureHint")}</p>
            ) : null}
            <div className="grid gap-3 sm:grid-cols-3">
              <div className="space-y-1">
                <Label htmlFor="tv-host">{t("tv.host")}</Label>
                <Input
                  id="tv-host"
                  value={draft.host ?? ""}
                  placeholder="192.168.1.52"
                  onChange={(event) => setDraft({ ...draft, host: event.target.value })}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="tv-mac">{t("tv.mac")}</Label>
                <Input
                  id="tv-mac"
                  value={draft.mac ?? ""}
                  placeholder="2c:d9:74:c2:d4:57"
                  onChange={(event) => setDraft({ ...draft, mac: event.target.value })}
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="tv-box">{t("tv.boxHost")}</Label>
                <Input
                  id="tv-box"
                  value={draft.boxHost ?? ""}
                  placeholder="192.168.1.153"
                  onChange={(event) => setDraft({ ...draft, boxHost: event.target.value })}
                />
              </div>
            </div>
            <Button
              size="sm"
              onClick={() => configMutation.mutate(draft)}
              disabled={configMutation.isPending}
            >
              {configMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
              {t("tv.save")}
            </Button>
          </div>
        ) : null}

        {status?.configured ? (
          <>
            <div className="flex flex-wrap gap-2">
              <Button
                variant={isOn ? "outline" : "default"}
                size="sm"
                onClick={() => powerMutation.mutate(isOn ? "off" : "on")}
                disabled={powerMutation.isPending}
              >
                {powerMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Power className="mr-2 h-4 w-4" />
                )}
                {isOn ? t("tv.powerOff") : t("tv.powerOn")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => boxMutation.mutate()}
                disabled={boxMutation.isPending}
              >
                {boxMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Home className="mr-2 h-4 w-4" />
                )}
                {t("tv.switchToBox")}
              </Button>
              {isOn ? (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => ambilightMutation.mutate()}
                  disabled={ambilightMutation.isPending}
                >
                  {ambilightMutation.isPending ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <Sparkles className="mr-2 h-4 w-4" />
                  )}
                  {status.ambilight?.power ? t("tv.ambilightOff") : t("tv.ambilightOn")}
                </Button>
              ) : null}
            </div>

            {/* Deep standby needs a network wake first, which is not instant —
                say so rather than letting the button look stuck. */}
            {isDeep ? (
              <p className="text-sm text-muted-foreground">{t("tv.deepStandbyHint")}</p>
            ) : null}

            {isOn && volume ? (
              <div className="space-y-2">
                <div className="flex items-center justify-between text-sm">
                  <span className="text-muted-foreground">{t("tv.volume")}</span>
                  <span className="tabular-nums text-foreground">
                    {shownVolume} / {volume.max}
                  </span>
                </div>
                <div className="flex items-center gap-3">
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={volume.muted ? t("tv.unmute") : t("tv.mute")}
                    onClick={() => volumeMutation.mutate({ muted: !volume.muted })}
                    disabled={volumeMutation.isPending}
                  >
                    {volume.muted ? (
                      <VolumeX className="h-4 w-4" />
                    ) : (
                      <Volume2 className="h-4 w-4" />
                    )}
                  </Button>
                  <Slider
                    value={[shownVolume]}
                    min={volume.min}
                    max={volume.max}
                    step={1}
                    onValueChange={([value]) => setVolumeDraft(value)}
                    onValueCommit={([value]) => volumeMutation.mutate({ level: value })}
                    className="flex-1"
                  />
                </div>
              </div>
            ) : null}

            {isOn ? (
              <div className="space-y-2">
                <span className="text-sm text-muted-foreground">{t("tv.remote")}</span>
                <div className="flex items-start gap-4">
                  <div className="grid grid-cols-3 grid-rows-3 gap-1">
                    {dpad.map(({ key, icon: Icon, className }) => (
                      <Button
                        key={key}
                        variant="outline"
                        size="icon"
                        className={className}
                        aria-label={key}
                        onClick={() => keyMutation.mutate(key)}
                      >
                        <Icon className="h-4 w-4" />
                      </Button>
                    ))}
                    <Button
                      variant="secondary"
                      size="icon"
                      className="col-start-2 row-start-2"
                      aria-label="confirm"
                      onClick={() => keyMutation.mutate("confirm")}
                    >
                      OK
                    </Button>
                  </div>
                  <div className="flex flex-wrap gap-1">
                    {(["home", "back", "source", "play_pause"] as TvKey[]).map((key) => (
                      <Button
                        key={key}
                        variant="outline"
                        size="sm"
                        onClick={() => keyMutation.mutate(key)}
                      >
                        {key.replace(/_/g, " ")}
                      </Button>
                    ))}
                  </div>
                </div>
              </div>
            ) : null}
          </>
        ) : null}
      </CardContent>
    </Card>
  );
}
