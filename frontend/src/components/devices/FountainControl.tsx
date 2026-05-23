import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { fountainApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { Label } from "@/components/ui/label";
import { toast } from "@/hooks/use-toast";
import {
  Droplets,
  Loader2,
  Power,
  Sun,
  Leaf,
  RefreshCw,
  AlertTriangle,
  Filter,
  Gauge,
} from "lucide-react";

interface FountainControlProps {
  deviceId: string;
}

export function FountainControl({ deviceId }: FountainControlProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: statusData, isLoading } = useQuery({
    queryKey: ["fountain", deviceId, "status"],
    queryFn: () => fountainApi.status(deviceId),
    refetchInterval: 10000,
  });

  const powerMutation = useMutation({
    mutationFn: (enabled: boolean) => fountainApi.power(deviceId, enabled),
    onSuccess: (_, enabled) => {
      queryClient.invalidateQueries({ queryKey: ["fountain", deviceId] });
      toast({
        title: enabled ? t("fountain.fountainOn") : t("fountain.fountainOff"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("common.error"),
        variant: "destructive",
      });
    },
  });

  const uvMutation = useMutation({
    mutationFn: (enabled: boolean) => fountainApi.setUV(deviceId, enabled),
    onSuccess: (_, enabled) => {
      queryClient.invalidateQueries({ queryKey: ["fountain", deviceId] });
      toast({
        title: enabled ? t("fountain.uvOn") : t("fountain.uvOff"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("common.error"),
        variant: "destructive",
      });
    },
  });

  const ecoModeMutation = useMutation({
    mutationFn: (mode: number) => fountainApi.setEcoMode(deviceId, mode),
    onSuccess: (_, mode) => {
      queryClient.invalidateQueries({ queryKey: ["fountain", deviceId] });
      toast({
        title: mode === 0 ? t("fountain.ecoModeOff") : t("fountain.ecoModeOn", { mode }),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("common.error"),
        variant: "destructive",
      });
    },
  });

  const resetWaterMutation = useMutation({
    mutationFn: () => fountainApi.resetWater(deviceId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["fountain", deviceId] });
      toast({ title: t("fountain.waterCounterReset") });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("common.error"),
        variant: "destructive",
      });
    },
  });

  const resetFilterMutation = useMutation({
    mutationFn: () => fountainApi.resetFilter(deviceId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["fountain", deviceId] });
      toast({ title: t("fountain.filterCounterReset") });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("common.error"),
        variant: "destructive",
      });
    },
  });

  const resetPumpMutation = useMutation({
    mutationFn: () => fountainApi.resetPump(deviceId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["fountain", deviceId] });
      toast({ title: t("fountain.pumpCounterReset") });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("common.error"),
        variant: "destructive",
      });
    },
  });

  const parsedStatus = statusData?.parsed_status as
    | {
        power?: boolean;
        uv_enabled?: boolean;
        uv_runtime?: number; // in seconds, > 0 = UV active
        eco_mode?: number; // 0 = off, 1 = mode 1, 2 = mode 2
        water_level?: string;
        filter_life?: number; // in minutes
        pump_time?: number; // in minutes
        water_time?: number;
      }
    | undefined;

  // UV is considered active if uv_runtime > 0, otherwise fallback on uv_enabled
  const isUvActive = (parsedStatus?.uv_runtime ?? 0) > 0 || (parsedStatus?.uv_enabled ?? false);

  const formatSeconds = (seconds: number): string => {
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) {
      const mins = Math.floor(seconds / 60);
      const secs = seconds % 60;
      return secs > 0 ? `${mins}min ${secs}s` : `${mins}min`;
    }
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    return mins > 0 ? `${hours}h ${mins}min` : `${hours}h`;
  };

  const formatMinutes = (minutes: number): string => {
    if (minutes < 60) return `${minutes} min`;
    if (minutes < 1440) return `${Math.floor(minutes / 60)}h ${minutes % 60}min`;
    const days = Math.floor(minutes / 1440);
    const hours = Math.floor((minutes % 1440) / 60);
    return `${days}j ${hours}h`;
  };

  const isAnyMutating =
    powerMutation.isPending || uvMutation.isPending || ecoModeMutation.isPending;

  if (isLoading) {
    return (
      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <Skeleton className="h-6 w-37.5" />
          </CardHeader>
          <CardContent>
            <Skeleton className="h-50 w-full" />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <Skeleton className="h-6 w-37.5" />
          </CardHeader>
          <CardContent>
            <Skeleton className="h-50 w-full" />
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="grid gap-4 md:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Droplets className="h-5 w-5" />
            {t("fountain.controls")}
          </CardTitle>
          <CardDescription>{t("fountain.controlsDescription")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-muted">
                <Power className="h-5 w-5 text-muted-foreground" />
              </div>
              <div>
                <Label>{t("fountain.power")}</Label>
                <p className="text-sm text-muted-foreground">{t("fountain.powerDescription")}</p>
              </div>
            </div>
            <Switch
              checked={parsedStatus?.power ?? false}
              onCheckedChange={(checked) => powerMutation.mutate(checked)}
              disabled={isAnyMutating}
            />
          </div>

          <Separator />

          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div
                className={`flex h-10 w-10 items-center justify-center rounded-lg ${isUvActive ? "bg-yellow-200" : "bg-yellow-100"}`}
              >
                <Sun
                  className={`h-5 w-5 ${isUvActive ? "text-slate-800 animate-pulse" : "text-slate-600"}`}
                />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <Label>{t("fountain.uvSterilization")}</Label>
                  {isUvActive && (
                    <Badge variant="success" className="text-xs">
                      {t("fountain.uvActive")}
                    </Badge>
                  )}
                </div>
                <p className="text-sm text-muted-foreground">
                  {isUvActive
                    ? t("fountain.uvRuntime", {
                        time: formatSeconds(parsedStatus?.uv_runtime ?? 0),
                      })
                    : t("fountain.uvDescription")}
                </p>
              </div>
            </div>
            <Switch
              checked={isUvActive}
              onCheckedChange={(checked) => uvMutation.mutate(checked)}
              disabled={isAnyMutating}
            />
          </div>

          <Separator />

          <div className="space-y-3">
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-muted">
                <Leaf className="h-5 w-5 text-muted-foreground" />
              </div>
              <div>
                <Label>{t("fountain.ecoMode")}</Label>
                <p className="text-sm text-muted-foreground">{t("fountain.ecoModeDescription")}</p>
              </div>
            </div>
            <div className="flex gap-2">
              <Button
                variant={parsedStatus?.eco_mode === 1 ? "default" : "outline"}
                size="sm"
                onClick={() => ecoModeMutation.mutate(1)}
                disabled={isAnyMutating}
                className="flex-1"
              >
                {t("fountain.mode1")}
              </Button>
              <Button
                variant={parsedStatus?.eco_mode === 2 ? "default" : "outline"}
                size="sm"
                onClick={() => ecoModeMutation.mutate(2)}
                disabled={isAnyMutating}
                className="flex-1"
              >
                {t("fountain.mode2")}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("fountain.statusMaintenance")}</CardTitle>
          <CardDescription>{t("fountain.statusMaintenanceDescription")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Droplets className="h-4 w-4 text-muted-foreground" />
                <span className="text-sm font-medium">{t("fountain.waterLevel")}</span>
              </div>
              <Badge
                variant={
                  parsedStatus?.water_level === "low"
                    ? "destructive"
                    : parsedStatus?.water_level === "medium"
                      ? "warning"
                      : "success"
                }
              >
                {parsedStatus?.water_level === "low"
                  ? t("fountain.waterLevelLow")
                  : parsedStatus?.water_level === "medium"
                    ? t("fountain.waterLevelMedium")
                    : t("fountain.waterLevelOk")}
              </Badge>
            </div>
            {parsedStatus?.water_level === "low" && (
              <div className="flex items-center gap-2 text-sm text-destructive">
                <AlertTriangle className="h-4 w-4" />
                {t("fountain.addWater")}
              </div>
            )}
          </div>

          <Separator />

          {parsedStatus?.filter_life !== undefined && (
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Filter className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm font-medium">{t("fountain.filterTime")}</span>
                </div>
                <span className="text-sm font-mono">{formatMinutes(parsedStatus.filter_life)}</span>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => resetFilterMutation.mutate()}
                disabled={resetFilterMutation.isPending}
                className="w-full"
              >
                {resetFilterMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCw className="mr-2 h-4 w-4" />
                )}
                {t("fountain.resetFilterChanged")}
              </Button>
            </div>
          )}

          <Separator />

          {parsedStatus?.pump_time !== undefined && (
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Gauge className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm font-medium">{t("fountain.pumpTime")}</span>
                </div>
                <span className="text-sm font-mono">{formatMinutes(parsedStatus.pump_time)}</span>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => resetPumpMutation.mutate()}
                disabled={resetPumpMutation.isPending}
                className="w-full"
              >
                {resetPumpMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCw className="mr-2 h-4 w-4" />
                )}
                {t("fountain.resetPumpCleaned")}
              </Button>
            </div>
          )}

          <Separator />
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <Droplets className="h-4 w-4 text-muted-foreground" />
              <span className="text-sm font-medium">{t("fountain.freshWater")}</span>
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => resetWaterMutation.mutate()}
              disabled={resetWaterMutation.isPending}
              className="w-full"
            >
              {resetWaterMutation.isPending ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="mr-2 h-4 w-4" />
              )}
              {t("fountain.waterChanged")}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
