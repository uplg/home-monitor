import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { merossApi, type MerossPlug } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "@/hooks/use-toast";
import {
  Plug,
  PlugZap,
  Wifi,
  WifiOff,
  Zap,
  Gauge,
  Activity,
  Info,
  Loader2,
  BellOff,
} from "lucide-react";

// ─── Detail page component ───

interface MerossPlugControlProps {
  deviceId: string;
}

export function MerossPlugControl({ deviceId }: MerossPlugControlProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: statusData, isLoading: isLoadingStatus } = useQuery({
    queryKey: ["meross-plug", deviceId],
    queryFn: () => merossApi.status(deviceId),
    refetchInterval: 3000,
  });

  const { data: electricityData } = useQuery({
    queryKey: ["meross-electricity", deviceId],
    queryFn: () => merossApi.electricity(deviceId),
    refetchInterval: 5000,
  });

  const { data: consumptionData } = useQuery({
    queryKey: ["meross-consumption", deviceId],
    queryFn: () => merossApi.consumption(deviceId),
    refetchInterval: 30000,
  });

  const toggleMutation = useMutation({
    mutationFn: (on: boolean) => merossApi.toggle(deviceId, on),
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: ["meross-plug", deviceId] });
      queryClient.invalidateQueries({ queryKey: ["meross-plugs"] });
      toast({
        title: data.on ? t("meross.plugOn") : t("meross.plugOff"),
        description: t("meross.powerChanged"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("meross.powerFailed"),
        variant: "destructive",
      });
    },
  });

  const dndMutation = useMutation({
    mutationFn: (enabled: boolean) => merossApi.dnd(deviceId, enabled),
    onSuccess: () => {
      toast({
        title: t("meross.dndUpdated"),
        description: t("meross.dndUpdatedDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("meross.dndFailed"),
        variant: "destructive",
      });
    },
  });

  if (isLoadingStatus) {
    return (
      <div className="space-y-4">
        <Card>
          <CardHeader>
            <Skeleton className="h-6 w-48" />
            <Skeleton className="h-4 w-64" />
          </CardHeader>
          <CardContent className="space-y-6">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </CardContent>
        </Card>
      </div>
    );
  }

  const status = statusData?.status;
  const device = statusData?.device;
  const isOnline = status?.online ?? false;
  const isOn = status?.on ?? false;

  if (!status || !device) {
    return (
      <Card>
        <CardContent className="py-8 text-center">
          <Plug className="mx-auto h-12 w-12 text-muted-foreground" />
          <p className="mt-4 text-muted-foreground">{t("meross.notFound")}</p>
        </CardContent>
      </Card>
    );
  }

  const elec = electricityData?.electricity;

  return (
    <div className="space-y-4">
      {/* Power Control Card */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div
                className={`flex h-12 w-12 items-center justify-center rounded-xl ${
                  isOn
                    ? "bg-emerald-100 text-emerald-600 dark:bg-emerald-950/50 dark:text-emerald-300"
                    : "bg-muted text-muted-foreground"
                }`}
              >
                {isOn ? <PlugZap className="h-6 w-6" /> : <Plug className="h-6 w-6" />}
              </div>
              <div>
                <CardTitle className="flex items-center gap-2">
                  {device.name}
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
                </CardTitle>
                <CardDescription>MSS310 • {device.id}</CardDescription>
              </div>
            </div>
            <Switch
              checked={isOn}
              onCheckedChange={(checked) => toggleMutation.mutate(checked)}
              disabled={!isOnline || toggleMutation.isPending}
            />
          </div>
        </CardHeader>
      </Card>

      {/* Real-time Electricity Card */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Zap className="h-5 w-5" />
            {t("meross.electricity")}
          </CardTitle>
          <CardDescription>{t("meross.electricityDescription")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-3 gap-4">
            <div className="text-center">
              <div className="flex items-center justify-center gap-1 text-muted-foreground text-sm mb-1">
                <Gauge className="h-4 w-4" />
                {t("meross.voltage")}
              </div>
              <p className="text-2xl font-bold">
                {elec ? parseFloat(elec.voltage).toFixed(1) : "--"}
                <span className="text-sm font-normal text-muted-foreground ml-0.5">V</span>
              </p>
            </div>
            <div className="text-center">
              <div className="flex items-center justify-center gap-1 text-muted-foreground text-sm mb-1">
                <Activity className="h-4 w-4" />
                {t("meross.current")}
              </div>
              <p className="text-2xl font-bold">
                {elec ? (parseFloat(elec.current) * 1000).toFixed(0) : "--"}
                <span className="text-sm font-normal text-muted-foreground ml-0.5">mA</span>
              </p>
            </div>
            <div className="text-center">
              <div className="flex items-center justify-center gap-1 text-muted-foreground text-sm mb-1">
                <Zap className="h-4 w-4" />
                {t("meross.power")}
              </div>
              <p className="text-2xl font-bold">
                {elec ? parseFloat(elec.power).toFixed(1) : "--"}
                <span className="text-sm font-normal text-muted-foreground ml-0.5">W</span>
              </p>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Consumption History Card */}
      {consumptionData?.consumption && consumptionData.consumption.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Activity className="h-5 w-5" />
              {t("meross.consumption")}
            </CardTitle>
            <CardDescription>{t("meross.consumptionDescription")}</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex items-center justify-between mb-4">
              <span className="text-sm text-muted-foreground">
                {t("meross.consumptionDays", { count: consumptionData.summary.days })}
              </span>
              <span className="text-lg font-bold">{consumptionData.summary.totalKwh} kWh</span>
            </div>
            <div className="space-y-1.5 max-h-48 overflow-y-auto">
              {consumptionData.consumption
                .slice(-7)
                .reverse()
                .map((entry) => (
                  <div key={entry.date} className="flex items-center justify-between text-sm">
                    <span className="text-muted-foreground">{entry.date}</span>
                    <span className="font-medium">{(entry.value / 1000).toFixed(3)} kWh</span>
                  </div>
                ))}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Device Info Card */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Info className="h-5 w-5" />
            {t("meross.deviceInfo")}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div>
              <span className="text-muted-foreground">{t("meross.model")}</span>
              <p className="font-medium">
                {status.hardware?.type?.toUpperCase() || t("common.unknown")}
              </p>
            </div>
            <div>
              <span className="text-muted-foreground">{t("meross.hwVersion")}</span>
              <p className="font-medium">{status.hardware?.version || t("common.unknown")}</p>
            </div>
            <div>
              <span className="text-muted-foreground">{t("meross.firmware")}</span>
              <p className="font-medium">{status.firmware?.version || t("common.unknown")}</p>
            </div>
            <div>
              <span className="text-muted-foreground">{t("meross.chip")}</span>
              <p className="font-medium">{status.hardware?.chipType || t("common.unknown")}</p>
            </div>
            <div>
              <span className="text-muted-foreground">{t("meross.mac")}</span>
              <p className="font-mono text-xs">{status.hardware?.mac || t("common.unknown")}</p>
            </div>
            <div>
              <span className="text-muted-foreground">{t("meross.ip")}</span>
              <p className="font-mono text-xs">{status.firmware?.innerIp || device.id}</p>
            </div>
            {status.wifi.signal !== null && (
              <div>
                <span className="text-muted-foreground">{t("meross.wifiSignal")}</span>
                <p className="font-medium">{status.wifi.signal}%</p>
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      {/* DND Mode Card */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <BellOff className="h-5 w-5" />
            {t("meross.dndMode")}
          </CardTitle>
          <CardDescription>{t("meross.dndDescription")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => dndMutation.mutate(true)}
              disabled={dndMutation.isPending}
            >
              {dndMutation.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {t("meross.ledOff")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => dndMutation.mutate(false)}
              disabled={dndMutation.isPending}
            >
              {t("meross.ledOn")}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* Offline warning */}
      {!isOnline && (
        <Card className="border-yellow-200 bg-yellow-50 dark:border-yellow-900 dark:bg-yellow-950/40">
          <CardContent className="py-4">
            <div className="flex items-center gap-3">
              <WifiOff className="h-5 w-5 text-yellow-600 dark:text-yellow-400" />
              <div>
                <p className="font-medium text-yellow-800 dark:text-yellow-200">
                  {t("meross.notConnected")}
                </p>
                <p className="text-sm text-yellow-700 dark:text-yellow-300">
                  {t("meross.notConnectedDescription")}
                </p>
              </div>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

// ─── Compact Dashboard Card ───

interface MerossPlugCardProps {
  plug: MerossPlug;
}

export function MerossPlugCard({ plug }: MerossPlugCardProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const toggleMutation = useMutation({
    mutationFn: (on: boolean) => merossApi.toggle(plug.id, on),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["meross-plugs"] });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("meross.powerFailed"),
        variant: "destructive",
      });
    },
  });

  // Poll electricity for this plug
  const { data: elecData } = useQuery({
    queryKey: ["meross-electricity", plug.id],
    queryFn: () => merossApi.electricity(plug.id),
    refetchInterval: 5000,
    enabled: plug.isOnline,
  });

  const isOn = plug.isOn;
  const isOnline = plug.isOnline;
  const elec = elecData?.electricity;

  return (
    <Card className="transition-shadow hover:shadow-lg">
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div
              className={`flex h-10 w-10 items-center justify-center rounded-lg ${
                isOn
                  ? "bg-emerald-100 text-emerald-600 dark:bg-emerald-950/50 dark:text-emerald-300"
                  : "bg-muted text-muted-foreground"
              }`}
            >
              {isOn ? <PlugZap className="h-5 w-5" /> : <Plug className="h-5 w-5" />}
            </div>
            <div>
              <Link to={`/meross/${plug.id}`}>
                <CardTitle className="text-base hover:underline cursor-pointer">
                  {plug.name}
                </CardTitle>
              </Link>
              <CardDescription className="text-xs">{plug.ip}</CardDescription>
            </div>
          </div>
          <Switch
            checked={isOn}
            onCheckedChange={(checked) => toggleMutation.mutate(checked)}
            disabled={!isOnline || toggleMutation.isPending}
          />
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {/* Connection status */}
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
          {elec && (
            <span className="text-muted-foreground font-medium">
              {parseFloat(elec.power).toFixed(1)}W
            </span>
          )}
        </div>

        {/* Electricity mini stats */}
        {elec && isOnline && (
          <div className="grid grid-cols-3 gap-2 text-center text-xs">
            <div>
              <span className="text-muted-foreground">{t("meross.voltage")}</span>
              <p className="font-medium">{parseFloat(elec.voltage).toFixed(1)}V</p>
            </div>
            <div>
              <span className="text-muted-foreground">{t("meross.current")}</span>
              <p className="font-medium">{(parseFloat(elec.current) * 1000).toFixed(0)}mA</p>
            </div>
            <div>
              <span className="text-muted-foreground">{t("meross.power")}</span>
              <p className="font-medium">{parseFloat(elec.power).toFixed(1)}W</p>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
