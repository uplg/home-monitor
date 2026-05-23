import React, { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { nabaztagApi, type NabaztagConfig } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { toast } from "@/hooks/use-toast";
import {
  Rabbit,
  Wifi,
  WifiOff,
  Moon,
  Sun,
  Ear,
  Lightbulb,
  Volume2,
  VolumeX,
  Play,
  MessageSquare,
  Terminal,
  Zap,
  Settings,
  RefreshCw,
  RotateCcw,
  Clock,
  Sparkles,
  Activity,
  Loader2,
  Send,
  Eraser,
  Info,
} from "lucide-react";

// ─── Dashboard Card (compact) ───

export function NabaztagCard() {
  const { t } = useTranslation();

  const { data: configData, isLoading: isLoadingConfig } = useQuery({
    queryKey: ["nabaztag-config"],
    queryFn: nabaztagApi.getConfig,
    staleTime: 60000,
    retry: 1,
  });

  const { data: statusData } = useQuery({
    queryKey: ["nabaztag-status"],
    queryFn: nabaztagApi.getStatus,
    refetchInterval: 10000,
    enabled: !!configData?.config?.host,
    retry: 1,
  });

  const isConfigured = !!configData?.config?.host;
  const isOnline = !!statusData?.success;
  const config = configData?.config;

  if (isLoadingConfig) {
    return (
      <Card className="transition-shadow hover:shadow-lg">
        <CardHeader className="pb-2">
          <div className="flex items-center gap-3">
            <Skeleton className="h-10 w-10 rounded-lg" />
            <div className="space-y-1">
              <Skeleton className="h-4 w-24" />
              <Skeleton className="h-3 w-32" />
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <Skeleton className="h-8 w-full" />
        </CardContent>
      </Card>
    );
  }

  // Don't render card if not configured
  if (!isConfigured) {
    return null;
  }

  return (
    <Card className="transition-shadow hover:shadow-lg">
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div
              className={`flex h-10 w-10 items-center justify-center rounded-lg ${
                isOnline ? "bg-violet-100 text-violet-600" : "bg-gray-100 text-gray-400"
              }`}
            >
              <Rabbit className="h-5 w-5" />
            </div>
            <div>
              <Link to="/nabaztag">
                <CardTitle className="text-base hover:underline cursor-pointer">
                  {config?.name || t("nabaztag.title")}
                </CardTitle>
              </Link>
              <CardDescription className="text-xs">{config?.host}</CardDescription>
            </div>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex items-center justify-between text-xs">
          {isOnline ? (
            <Badge variant="success" className="text-xs">
              <Wifi className="mr-1 h-2.5 w-2.5" />
              {t("common.connected")}
            </Badge>
          ) : (
            <Badge variant="secondary" className="text-xs">
              <WifiOff className="mr-1 h-2.5 w-2.5" />
              {t("common.disconnected")}
            </Badge>
          )}
          {config?.tempoEnabled && (
            <Badge variant="outline" className="text-xs">
              <Zap className="mr-1 h-2.5 w-2.5" />
              Tempo
            </Badge>
          )}
        </div>
        <Link to="/nabaztag">
          <Button variant="default" size="sm" className="w-full">
            {t("common.manage")}
          </Button>
        </Link>
      </CardContent>
    </Card>
  );
}

// ─── Full Control Component (used on dedicated page) ───

export function NabaztagFullControl() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  // ---- State ----
  const [configForm, setConfigForm] = useState<NabaztagConfig | null>(null);
  const [earLeft, setEarLeft] = useState(0);
  const [earRight, setEarRight] = useState(0);
  const [ledNose, setLedNose] = useState("#0000ff");
  const [ledLeft, setLedLeft] = useState("#0000ff");
  const [ledCenter, setLedCenter] = useState("#0000ff");
  const [ledRight, setLedRight] = useState("#0000ff");
  const [ledBase, setLedBase] = useState("#000088");
  const [ledBreathing, setLedBreathing] = useState(false);
  const [playUrlValue, setPlayUrlValue] = useState("");
  const [ttsValue, setTtsValue] = useState("");
  const [forthCode, setForthCode] = useState("");
  const [forthOutput, setForthOutput] = useState("");
  const [infoService, setInfoService] = useState("weather");
  const [infoValue, setInfoValue] = useState(0);

  // ---- Queries ----
  const { data: configData, isLoading: isLoadingConfig } = useQuery({
    queryKey: ["nabaztag-config"],
    queryFn: nabaztagApi.getConfig,
    staleTime: 60000,
  });

  const { data: statusData, isLoading: isLoadingStatus } = useQuery({
    queryKey: ["nabaztag-status"],
    queryFn: nabaztagApi.getStatus,
    refetchInterval: 10000,
    enabled: !!configData?.config?.host,
    retry: 1,
  });

  const { data: earsData } = useQuery({
    queryKey: ["nabaztag-ears"],
    queryFn: nabaztagApi.getEars,
    refetchInterval: 15000,
    enabled: !!configData?.config?.host,
    retry: 1,
  });

  const isConfigured = !!configData?.config?.host;
  const isOnline = !!statusData?.success;

  // ---- Mutations ----
  const configMutation = useMutation({
    mutationFn: (config: NabaztagConfig) => nabaztagApi.updateConfig(config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["nabaztag-config"] });
      queryClient.invalidateQueries({ queryKey: ["nabaztag-status"] });
      setConfigForm(null);
      toast({ title: t("nabaztag.configSaved") });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.configFailed"),
        variant: "destructive",
      });
    },
  });

  const sleepMutation = useMutation({
    mutationFn: nabaztagApi.sleep,
    onSuccess: () => toast({ title: t("nabaztag.sleepSent") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.actionFailed"),
        variant: "destructive",
      }),
  });

  const wakeupMutation = useMutation({
    mutationFn: nabaztagApi.wakeup,
    onSuccess: () => toast({ title: t("nabaztag.wakeUpSent") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.actionFailed"),
        variant: "destructive",
      }),
  });

  const earMutation = useMutation({
    mutationFn: nabaztagApi.moveEar,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["nabaztag-ears"] });
      toast({ title: t("nabaztag.earMoved") });
    },
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.earFailed"),
        variant: "destructive",
      }),
  });

  const ledsMutation = useMutation({
    mutationFn: nabaztagApi.setLeds,
    onSuccess: () => toast({ title: t("nabaztag.ledsSet") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.ledsFailed"),
        variant: "destructive",
      }),
  });

  const clearLedsMutation = useMutation({
    mutationFn: nabaztagApi.clearLeds,
    onSuccess: () => toast({ title: t("nabaztag.ledsCleared") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.ledsFailed"),
        variant: "destructive",
      }),
  });

  const playUrlMutation = useMutation({
    mutationFn: nabaztagApi.playUrl,
    onSuccess: () => toast({ title: t("nabaztag.soundPlaying") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.soundFailed"),
        variant: "destructive",
      }),
  });

  const sayMutation = useMutation({
    mutationFn: nabaztagApi.say,
    onSuccess: () => toast({ title: t("nabaztag.soundPlaying") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.soundFailed"),
        variant: "destructive",
      }),
  });

  const stopMutation = useMutation({
    mutationFn: nabaztagApi.stop,
    onSuccess: () => toast({ title: t("nabaztag.stopped") }),
  });

  const midiMutation = useMutation({
    mutationFn: (type: "communication" | "ack" | "abort" | "ministop") => {
      switch (type) {
        case "communication":
          return nabaztagApi.soundCommunication();
        case "ack":
          return nabaztagApi.soundAck();
        case "abort":
          return nabaztagApi.soundAbort();
        case "ministop":
          return nabaztagApi.soundMinistop();
      }
    },
    onSuccess: () => toast({ title: t("nabaztag.soundPlaying") }),
  });

  const infoMutation = useMutation({
    mutationFn: (req: { service: string; value: number }) => nabaztagApi.setInfoService(req),
    onSuccess: () => toast({ title: t("nabaztag.serviceSet") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.serviceFailed"),
        variant: "destructive",
      }),
  });

  const clearInfoMutation = useMutation({
    mutationFn: nabaztagApi.clearInfo,
    onSuccess: () => toast({ title: t("nabaztag.servicesCleared") }),
  });

  const actionMutation = useMutation({
    mutationFn: (action: "taichi" | "surprise" | "reboot" | "updateTime") => {
      switch (action) {
        case "taichi":
          return nabaztagApi.taichi();
        case "surprise":
          return nabaztagApi.surprise();
        case "reboot":
          return nabaztagApi.reboot();
        case "updateTime":
          return nabaztagApi.updateTime();
      }
    },
    onSuccess: () => toast({ title: t("nabaztag.actionSent") }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.actionFailed"),
        variant: "destructive",
      }),
  });

  const forthMutation = useMutation({
    mutationFn: nabaztagApi.executeForth,
    onSuccess: (data) => {
      setForthOutput(data.output);
      toast({ title: t("nabaztag.forthExecuted") });
    },
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.forthFailed"),
        variant: "destructive",
      }),
  });

  const tempoPushMutation = useMutation({
    mutationFn: (forceRefresh: boolean) => nabaztagApi.pushTempo(forceRefresh),
    onSuccess: (data) => toast({ title: t("nabaztag.tempoPushed"), description: data.message }),
    onError: (error) =>
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.tempoFailed"),
        variant: "destructive",
      }),
  });

  // ---- Render ----

  if (isLoadingConfig) {
    return (
      <div className="space-y-4">
        <Card>
          <CardHeader>
            <Skeleton className="h-6 w-48" />
            <Skeleton className="h-4 w-64" />
          </CardHeader>
          <CardContent className="space-y-4">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </CardContent>
        </Card>
      </div>
    );
  }

  const config = configData?.config;
  const editingConfig = configForm ?? config ?? { host: "", name: "Nabaztag", tempoEnabled: false };

  return (
    <div className="space-y-4">
      {/* Configuration Card */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Settings className="h-5 w-5" />
            {t("nabaztag.configure")}
          </CardTitle>
          <CardDescription>{t("nabaztag.subtitle")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div className="space-y-1">
              <label className="text-sm font-medium">{t("nabaztag.host")}</label>
              <Input
                value={editingConfig.host}
                placeholder={t("nabaztag.hostPlaceholder")}
                onChange={(e) => setConfigForm({ ...editingConfig, host: e.target.value })}
              />
            </div>
            <div className="space-y-1">
              <label className="text-sm font-medium">{t("nabaztag.name")}</label>
              <Input
                value={editingConfig.name}
                placeholder={t("nabaztag.namePlaceholder")}
                onChange={(e) => setConfigForm({ ...editingConfig, name: e.target.value })}
              />
            </div>
          </div>
          <div className="flex items-center justify-between">
            <div>
              <span className="text-sm font-medium">{t("nabaztag.tempoEnabled")}</span>
              <p className="text-xs text-muted-foreground">
                {t("nabaztag.tempoEnabledDescription")}
              </p>
            </div>
            <Switch
              checked={editingConfig.tempoEnabled}
              onCheckedChange={(checked) =>
                setConfigForm({ ...editingConfig, tempoEnabled: checked })
              }
            />
          </div>
          {configForm && (
            <div className="flex gap-2">
              <Button
                onClick={() => configMutation.mutate(configForm)}
                disabled={configMutation.isPending || !configForm.host.trim()}
              >
                {configMutation.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                {t("common.save")}
              </Button>
              <Button variant="outline" onClick={() => setConfigForm(null)}>
                {t("common.cancel")}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>

      {!isConfigured && (
        <Card className="border-yellow-200 bg-yellow-50">
          <CardContent className="py-6 text-center">
            <Rabbit className="mx-auto h-12 w-12 text-yellow-600" />
            <p className="mt-4 font-medium text-yellow-800">{t("nabaztag.notConfigured")}</p>
            <p className="mt-1 text-sm text-yellow-700">{t("nabaztag.notConfiguredHint")}</p>
          </CardContent>
        </Card>
      )}

      {isConfigured && (
        <>
          {/* Status & Sleep/Wake */}
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    <Info className="h-5 w-5" />
                    {t("nabaztag.status")}
                  </CardTitle>
                  <CardDescription>{t("nabaztag.statusDescription")}</CardDescription>
                </div>
                <div className="flex items-center gap-2">
                  {isOnline ? (
                    <Badge variant="success">
                      <Wifi className="mr-1 h-3 w-3" />
                      {t("common.connected")}
                    </Badge>
                  ) : (
                    <Badge variant="secondary">
                      <WifiOff className="mr-1 h-3 w-3" />
                      {t("common.disconnected")}
                    </Badge>
                  )}
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => sleepMutation.mutate()}
                  disabled={sleepMutation.isPending}
                >
                  {sleepMutation.isPending ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <Moon className="mr-2 h-4 w-4" />
                  )}
                  {t("nabaztag.sleep")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => wakeupMutation.mutate()}
                  disabled={wakeupMutation.isPending}
                >
                  {wakeupMutation.isPending ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <Sun className="mr-2 h-4 w-4" />
                  )}
                  {t("nabaztag.wakeUp")}
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => {
                    queryClient.invalidateQueries({ queryKey: ["nabaztag-status"] });
                    queryClient.invalidateQueries({ queryKey: ["nabaztag-ears"] });
                  }}
                >
                  <RefreshCw className={`h-4 w-4 ${isLoadingStatus ? "animate-spin" : ""}`} />
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Ears */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Ear className="h-5 w-5" />
                {t("nabaztag.ears")}
              </CardTitle>
              <CardDescription>{t("nabaztag.earsDescription")}</CardDescription>
            </CardHeader>
            <CardContent>
              {earsData?.ears && (
                <p className="mb-3 text-sm text-muted-foreground">
                  {t("common.current")}: L={earsData.ears.left} R={earsData.ears.right}
                </p>
              )}
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <div className="space-y-2">
                  <label className="text-sm font-medium">{t("nabaztag.leftEar")}</label>
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      min={0}
                      max={16}
                      value={earLeft}
                      onChange={(e) => setEarLeft(Number(e.target.value))}
                      className="w-20"
                    />
                    <Button
                      size="sm"
                      onClick={() =>
                        earMutation.mutate({ ear: 0, position: earLeft, direction: 0 })
                      }
                      disabled={earMutation.isPending}
                    >
                      {t("nabaztag.moveEar")}
                    </Button>
                  </div>
                </div>
                <div className="space-y-2">
                  <label className="text-sm font-medium">{t("nabaztag.rightEar")}</label>
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      min={0}
                      max={16}
                      value={earRight}
                      onChange={(e) => setEarRight(Number(e.target.value))}
                      className="w-20"
                    />
                    <Button
                      size="sm"
                      onClick={() =>
                        earMutation.mutate({ ear: 1, position: earRight, direction: 0 })
                      }
                      disabled={earMutation.isPending}
                    >
                      {t("nabaztag.moveEar")}
                    </Button>
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* LEDs */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Lightbulb className="h-5 w-5" />
                {t("nabaztag.leds")}
              </CardTitle>
              <CardDescription>{t("nabaztag.ledsDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-5">
                <div className="space-y-1">
                  <label className="text-xs font-medium">{t("nabaztag.nose")}</label>
                  <input
                    type="color"
                    value={ledNose}
                    onChange={(e) => setLedNose(e.target.value)}
                    className="h-10 w-full cursor-pointer rounded border"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-xs font-medium">{t("nabaztag.leftLed")}</label>
                  <input
                    type="color"
                    value={ledLeft}
                    onChange={(e) => setLedLeft(e.target.value)}
                    className="h-10 w-full cursor-pointer rounded border"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-xs font-medium">{t("nabaztag.centerLed")}</label>
                  <input
                    type="color"
                    value={ledCenter}
                    onChange={(e) => setLedCenter(e.target.value)}
                    className="h-10 w-full cursor-pointer rounded border"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-xs font-medium">{t("nabaztag.rightLed")}</label>
                  <input
                    type="color"
                    value={ledRight}
                    onChange={(e) => setLedRight(e.target.value)}
                    className="h-10 w-full cursor-pointer rounded border"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-xs font-medium">{t("nabaztag.baseLed")}</label>
                  <input
                    type="color"
                    value={ledBase}
                    onChange={(e) => setLedBase(e.target.value)}
                    className="h-10 w-full cursor-pointer rounded border"
                  />
                </div>
              </div>
              <div className="flex items-center gap-3">
                <Switch checked={ledBreathing} onCheckedChange={setLedBreathing} />
                <span className="text-sm">{t("nabaztag.breathing")}</span>
              </div>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  onClick={() =>
                    ledsMutation.mutate({
                      nose: ledNose,
                      left: ledLeft,
                      center: ledCenter,
                      right: ledRight,
                      base: ledBase,
                      breathing: ledBreathing,
                    })
                  }
                  disabled={ledsMutation.isPending}
                >
                  {ledsMutation.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                  {t("nabaztag.setLeds")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => clearLedsMutation.mutate()}
                  disabled={clearLedsMutation.isPending}
                >
                  <Eraser className="mr-2 h-4 w-4" />
                  {t("nabaztag.clearLeds")}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Sound */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Volume2 className="h-5 w-5" />
                {t("nabaztag.sound")}
              </CardTitle>
              <CardDescription>{t("nabaztag.soundDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {/* Play URL */}
              <div className="space-y-2">
                <label className="text-sm font-medium">{t("nabaztag.playUrl")}</label>
                <div className="flex gap-2">
                  <Input
                    value={playUrlValue}
                    placeholder={t("nabaztag.urlPlaceholder")}
                    onChange={(e) => setPlayUrlValue(e.target.value)}
                    className="flex-1"
                  />
                  <Button
                    size="sm"
                    onClick={() => {
                      if (playUrlValue.trim()) playUrlMutation.mutate(playUrlValue.trim());
                    }}
                    disabled={playUrlMutation.isPending || !playUrlValue.trim()}
                  >
                    <Play className="mr-2 h-4 w-4" />
                    {t("nabaztag.play")}
                  </Button>
                </div>
              </div>

              {/* TTS */}
              <div className="space-y-2">
                <label className="text-sm font-medium">{t("nabaztag.tts")}</label>
                <div className="flex gap-2">
                  <Input
                    value={ttsValue}
                    placeholder={t("nabaztag.ttsPlaceholder")}
                    onChange={(e) => setTtsValue(e.target.value)}
                    className="flex-1"
                  />
                  <Button
                    size="sm"
                    onClick={() => {
                      if (ttsValue.trim()) sayMutation.mutate(ttsValue.trim());
                    }}
                    disabled={sayMutation.isPending || !ttsValue.trim()}
                  >
                    <MessageSquare className="mr-2 h-4 w-4" />
                    {t("nabaztag.speak")}
                  </Button>
                </div>
              </div>

              {/* MIDI + Stop */}
              <div className="space-y-2">
                <label className="text-sm font-medium">{t("nabaztag.midiSounds")}</label>
                <div className="flex flex-wrap gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => midiMutation.mutate("communication")}
                    disabled={midiMutation.isPending}
                  >
                    {t("nabaztag.communication")}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => midiMutation.mutate("ack")}
                    disabled={midiMutation.isPending}
                  >
                    {t("nabaztag.ack")}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => midiMutation.mutate("abort")}
                    disabled={midiMutation.isPending}
                  >
                    {t("nabaztag.abort")}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => midiMutation.mutate("ministop")}
                    disabled={midiMutation.isPending}
                  >
                    {t("nabaztag.ministop")}
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={() => stopMutation.mutate()}
                    disabled={stopMutation.isPending}
                  >
                    <VolumeX className="mr-2 h-4 w-4" />
                    {t("nabaztag.stopAll")}
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* Info Services */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Activity className="h-5 w-5" />
                {t("nabaztag.infoServices")}
              </CardTitle>
              <CardDescription>{t("nabaztag.infoServicesDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="flex flex-wrap items-end gap-3">
                <div className="space-y-1">
                  <label className="text-xs font-medium">{t("nabaztag.service")}</label>
                  <select
                    value={infoService}
                    onChange={(e) => setInfoService(e.target.value)}
                    className="h-9 rounded-md border bg-background px-3 text-sm"
                  >
                    <option value="weather">weather</option>
                    <option value="pollution">pollution</option>
                    <option value="traffic">traffic</option>
                    <option value="stock">stock</option>
                    <option value="mail">mail</option>
                    <option value="service4">service4</option>
                    <option value="service5">service5</option>
                    <option value="nose">nose</option>
                  </select>
                </div>
                <div className="space-y-1">
                  <label className="text-xs font-medium">{t("nabaztag.value")}</label>
                  <Input
                    type="number"
                    min={-1}
                    max={10}
                    value={infoValue}
                    onChange={(e) => setInfoValue(Number(e.target.value))}
                    className="w-20"
                  />
                </div>
                <Button
                  size="sm"
                  onClick={() => infoMutation.mutate({ service: infoService, value: infoValue })}
                  disabled={infoMutation.isPending}
                >
                  {t("nabaztag.setService")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => clearInfoMutation.mutate()}
                  disabled={clearInfoMutation.isPending}
                >
                  <Eraser className="mr-2 h-4 w-4" />
                  {t("nabaztag.clearServices")}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Quick Actions */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Sparkles className="h-5 w-5" />
                {t("nabaztag.actions")}
              </CardTitle>
              <CardDescription>{t("nabaztag.actionsDescription")}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="flex flex-wrap gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => actionMutation.mutate("taichi")}
                  disabled={actionMutation.isPending}
                >
                  {t("nabaztag.taichi")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => actionMutation.mutate("surprise")}
                  disabled={actionMutation.isPending}
                >
                  <Sparkles className="mr-2 h-4 w-4" />
                  {t("nabaztag.surprise")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => actionMutation.mutate("updateTime")}
                  disabled={actionMutation.isPending}
                >
                  <Clock className="mr-2 h-4 w-4" />
                  {t("nabaztag.updateTime")}
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => actionMutation.mutate("reboot")}
                  disabled={actionMutation.isPending}
                >
                  <RotateCcw className="mr-2 h-4 w-4" />
                  {t("nabaztag.reboot")}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Tempo Push */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Zap className="h-5 w-5" />
                {t("nabaztag.tempo")}
              </CardTitle>
              <CardDescription>{t("nabaztag.tempoDescription")}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  onClick={() => tempoPushMutation.mutate(false)}
                  disabled={tempoPushMutation.isPending}
                >
                  {tempoPushMutation.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                  <Send className="mr-2 h-4 w-4" />
                  {t("nabaztag.pushTempo")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => tempoPushMutation.mutate(true)}
                  disabled={tempoPushMutation.isPending}
                >
                  <RefreshCw className="mr-2 h-4 w-4" />
                  {t("nabaztag.pushTempoRefresh")}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Forth Console */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Terminal className="h-5 w-5" />
                {t("nabaztag.forth")}
              </CardTitle>
              <CardDescription>{t("nabaztag.forthDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <Textarea
                value={forthCode}
                onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) =>
                  setForthCode(e.target.value)
                }
                placeholder={t("nabaztag.forthPlaceholder")}
                rows={3}
                className="font-mono text-sm"
              />
              <Button
                size="sm"
                onClick={() => {
                  if (forthCode.trim()) forthMutation.mutate(forthCode.trim());
                }}
                disabled={forthMutation.isPending || !forthCode.trim()}
              >
                {forthMutation.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                <Terminal className="mr-2 h-4 w-4" />
                {t("nabaztag.execute")}
              </Button>
              {forthOutput && (
                <div className="rounded-md bg-slate-900 p-3 text-sm text-slate-100">
                  <p className="mb-1 text-xs font-medium text-slate-400">
                    {t("nabaztag.forthOutput")}
                  </p>
                  <pre className="whitespace-pre-wrap font-mono text-xs">{forthOutput}</pre>
                </div>
              )}
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
