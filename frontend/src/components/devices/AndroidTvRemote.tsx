import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  CornerUpLeft,
  Home,
  Loader2,
  Menu,
  Moon,
  Pause,
  Play,
  Power,
  Search,
  Settings2,
  SkipBack,
  Upload,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from "lucide-react";
import {
  androidTvApi,
  type AndroidKey,
  type AndroidTvConfig,
  type AndroidTvStatus,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "@/hooks/use-toast";
import { CONFIRM, FAILURE, haptic, TAP } from "@/lib/haptics";

/** Apps worth a shortcut, keyed by the packages actually on the box. */
const SHORTCUTS: Array<{ package: string; label: string }> = [
  { package: "org.smarttube.beta", label: "SmartTube" },
  { package: "studio.kahn.iris.tv", label: "Iris" },
];

/**
 * A real remote rather than a list of buttons: circular D-pad with a centre
 * OK, media transport, and a volume rocker. Sized in `rem` off a single
 * container so it scales from a phone to a wide dashboard column without
 * breaking its proportions.
 */
export function AndroidTvRemote() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showConfig, setShowConfig] = useState(false);
  const [draft, setDraft] = useState<AndroidTvConfig>({});

  const statusQuery = useQuery({
    queryKey: ["androidtv-status"],
    queryFn: androidTvApi.status,
    staleTime: 15_000,
    refetchInterval: 60_000,
  });

  const config = statusQuery.data?.config;
  const status: AndroidTvStatus | undefined = statusQuery.data?.status;

  useEffect(() => {
    if (config) setDraft(config);
  }, [config]);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["androidtv-status"] });
  const fail = (error: unknown) => {
    haptic(FAILURE);
    return toast({
      title: t("common.error"),
      description: error instanceof Error ? error.message : String(error),
      variant: "destructive",
    });
  };

  const keyMutation = useMutation({
    mutationFn: (key: AndroidKey) => androidTvApi.sendKey(key),
    onError: fail,
  });

  const powerMutation = useMutation({
    mutationFn: (wake: boolean) => (wake ? androidTvApi.wake() : androidTvApi.sleep()),
    onSuccess: invalidate,
    onError: fail,
  });

  const launchMutation = useMutation({
    mutationFn: (pkg: string) => androidTvApi.launch(pkg),
    onSuccess: (response) => {
      invalidate();
      toast({ title: t("androidTv.launched"), description: response.message });
    },
    onError: fail,
  });

  const apkMutation = useMutation({
    mutationFn: (file: File) => androidTvApi.installApk(file),
    onSuccess: (response) => {
      invalidate();
      toast({ title: t("androidTv.apkInstalled"), description: response.message });
    },
    onError: fail,
  });

  const configMutation = useMutation({
    mutationFn: (next: AndroidTvConfig) => androidTvApi.setConfig(next),
    onSuccess: () => {
      invalidate();
      setShowConfig(false);
      toast({ title: t("androidTv.saved") });
    },
    onError: fail,
  });

  const press =
    (key: AndroidKey, pattern = TAP) =>
    () => {
      haptic(pattern);
      keyMutation.mutate(key);
    };
  const reachable = status?.reachable ?? false;
  const shortcuts = config?.favouriteApps?.length ? config.favouriteApps : SHORTCUTS;
  const currentShortcut = shortcuts.find((app) => app.package === status?.currentApp);

  /** Round key on the remote body. */
  const RemoteKey = ({
    onClick,
    label,
    children,
    tone = "default",
  }: {
    onClick: () => void;
    label: string;
    children: React.ReactNode;
    tone?: "default" | "accent" | "danger";
  }) => (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      disabled={!reachable}
      className={[
        "flex h-11 w-11 items-center justify-center rounded-full border transition",
        "active:scale-95 disabled:cursor-not-allowed disabled:opacity-40",
        tone === "accent"
          ? "border-primary/30 bg-primary/10 text-primary hover:bg-primary/20"
          : tone === "danger"
            ? "border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/20"
            : "border-border/70 bg-background/70 text-foreground hover:bg-muted",
      ].join(" ")}
    >
      {children}
    </button>
  );

  return (
    <Card className="border-border/60 bg-card/60">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-3">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-muted text-muted-foreground">
            <Play className="h-5 w-5" />
          </div>
          <div className="min-w-0">
            <CardTitle className="text-[1.1rem] tracking-[-0.02em]">
              {t("androidTv.title")}
            </CardTitle>
            <p className="truncate text-sm text-muted-foreground">
              {currentShortcut
                ? `${t("androidTv.playing")} · ${currentShortcut.label}`
                : t("androidTv.subtitle")}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {status?.configured ? (
            <Badge variant={reachable ? "default" : "secondary"}>
              {!reachable
                ? t("androidTv.unreachable")
                : status.awake
                  ? t("androidTv.awake")
                  : t("androidTv.asleep")}
            </Badge>
          ) : null}
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("androidTv.configure")}
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
              <p className="text-sm text-muted-foreground">{t("androidTv.configureHint")}</p>
            ) : null}
            <div className="space-y-1">
              <Label htmlFor="atv-host">{t("androidTv.host")}</Label>
              <Input
                id="atv-host"
                value={draft.host ?? ""}
                placeholder="192.168.1.153"
                onChange={(event) => setDraft({ ...draft, host: event.target.value })}
              />
            </div>
            <p className="text-xs text-muted-foreground">{t("androidTv.firstRunHint")}</p>
            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                onClick={() => configMutation.mutate(draft)}
                disabled={configMutation.isPending}
              >
                {configMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : null}
                {t("androidTv.save")}
              </Button>

              {/* Sideloading: the box is plain Android, so an APK just installs. */}
              <Button
                size="sm"
                variant="outline"
                asChild={!apkMutation.isPending}
                disabled={apkMutation.isPending}
              >
                {apkMutation.isPending ? (
                  <span className="inline-flex items-center">
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    {t("androidTv.installing")}
                  </span>
                ) : (
                  <label className="cursor-pointer">
                    <Upload className="mr-2 h-4 w-4" />
                    {t("androidTv.installApk")}
                    <input
                      type="file"
                      accept=".apk,application/vnd.android.package-archive"
                      className="hidden"
                      onChange={(event) => {
                        const file = event.target.files?.[0];
                        if (file) apkMutation.mutate(file);
                        event.target.value = "";
                      }}
                    />
                  </label>
                )}
              </Button>
            </div>
          </div>
        ) : null}

        {status?.configured ? (
          <div className="mx-auto w-full max-w-[19rem] space-y-5 rounded-[1.75rem] border border-border/60 bg-gradient-to-b from-background/80 to-muted/40 p-5 shadow-sm">
            {/* Power row */}
            <div className="flex items-center justify-between">
              <RemoteKey
                onClick={() => {
                  haptic(CONFIRM);
                  powerMutation.mutate(!status.awake);
                }}
                label={status.awake ? t("androidTv.sleep") : t("androidTv.wake")}
                tone={status.awake ? "danger" : "accent"}
              >
                {powerMutation.isPending ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : status.awake ? (
                  <Moon className="h-4 w-4" />
                ) : (
                  <Power className="h-4 w-4" />
                )}
              </RemoteKey>
              <RemoteKey onClick={press("search")} label="Search">
                <Search className="h-4 w-4" />
              </RemoteKey>
              <RemoteKey onClick={press("home")} label="Home">
                <Home className="h-4 w-4" />
              </RemoteKey>
              <RemoteKey onClick={press("back")} label="Back">
                <CornerUpLeft className="h-4 w-4" />
              </RemoteKey>
            </div>

            {/* D-pad: a real ring with a centre OK */}
            <div className="relative mx-auto aspect-square w-full max-w-[13rem]">
              <div className="absolute inset-0 rounded-full border border-border/70 bg-background/60 shadow-inner" />
              {(
                [
                  { key: "up", Icon: ChevronUp, pos: "left-1/2 top-2 -translate-x-1/2" },
                  { key: "down", Icon: ChevronDown, pos: "bottom-2 left-1/2 -translate-x-1/2" },
                  { key: "left", Icon: ChevronLeft, pos: "left-2 top-1/2 -translate-y-1/2" },
                  { key: "right", Icon: ChevronRight, pos: "right-2 top-1/2 -translate-y-1/2" },
                ] as const
              ).map(({ key, Icon, pos }) => (
                <button
                  key={key}
                  type="button"
                  aria-label={key}
                  disabled={!reachable}
                  onClick={press(key)}
                  className={`absolute ${pos} flex h-12 w-12 items-center justify-center rounded-full text-muted-foreground transition hover:bg-muted hover:text-foreground active:scale-95 disabled:cursor-not-allowed disabled:opacity-40`}
                >
                  <Icon className="h-6 w-6" />
                </button>
              ))}
              <button
                type="button"
                aria-label="OK"
                disabled={!reachable}
                onClick={press("ok", CONFIRM)}
                className="absolute left-1/2 top-1/2 flex h-[4.5rem] w-[4.5rem] -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border border-border/70 bg-card text-sm font-semibold tracking-wide shadow-sm transition hover:bg-muted active:scale-95 disabled:cursor-not-allowed disabled:opacity-40"
              >
                OK
              </button>
            </div>

            {/* Media transport */}
            <div className="flex items-center justify-center gap-3">
              <RemoteKey onClick={press("previous")} label="Previous">
                <SkipBack className="h-4 w-4" />
              </RemoteKey>
              <RemoteKey onClick={press("play_pause")} label="Play / Pause" tone="accent">
                <Pause className="h-4 w-4" />
              </RemoteKey>
              <RemoteKey onClick={press("next")} label="Next">
                <SkipForward className="h-4 w-4" />
              </RemoteKey>
              <RemoteKey onClick={press("menu")} label="Menu">
                <Menu className="h-4 w-4" />
              </RemoteKey>
            </div>

            {/* Volume rocker */}
            <div className="flex items-center justify-center gap-3">
              <RemoteKey onClick={press("volume_down")} label="Volume down">
                <Volume1 className="h-4 w-4" />
              </RemoteKey>
              <RemoteKey onClick={press("mute")} label="Mute">
                <VolumeX className="h-4 w-4" />
              </RemoteKey>
              <RemoteKey onClick={press("volume_up")} label="Volume up">
                <Volume2 className="h-4 w-4" />
              </RemoteKey>
            </div>

            {/* App shortcuts */}
            <div className="space-y-2 border-t border-border/60 pt-4">
              <span className="text-xs uppercase tracking-wide text-muted-foreground">
                {t("androidTv.apps")}
              </span>
              <div className="flex flex-wrap gap-2">
                {shortcuts.map((app) => (
                  <Button
                    key={app.package}
                    variant={status.currentApp === app.package ? "default" : "outline"}
                    size="sm"
                    className="rounded-full"
                    disabled={launchMutation.isPending}
                    onClick={() => {
                      haptic(CONFIRM);
                      launchMutation.mutate(app.package);
                    }}
                  >
                    {launchMutation.isPending && launchMutation.variables === app.package ? (
                      <Loader2 className="mr-2 h-3 w-3 animate-spin" />
                    ) : null}
                    {app.label}
                  </Button>
                ))}
              </div>
            </div>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
