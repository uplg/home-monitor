import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { devicesApi, hueLampsApi, merossApi, zigbeeLampsApi, type Device } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "@/hooks/use-toast";
import { HueLampCard } from "@/components/devices/HueLampControl";
import { ZigbeeLampCard, ZigbeePairingPanel } from "@/components/devices/ZigbeeLampControl";
import { BroadlinkClimateControl } from "@/components/devices/BroadlinkClimateControl";
import { MerossPlugCard } from "@/components/devices/MerossPlugControl";
import { TempoCard } from "@/components/devices/TempoCard";
import { NabaztagTile } from "@/components/devices/NabaztagTile";
// Nabaztag is disabled while the physical device is being repaired.
// import { NabaztagCard } from "@/components/devices/NabaztagControl";
import { DashboardSectionHeader } from "@/components/dashboard/DashboardSectionHeader";
import {
  Utensils,
  Droplets,
  Trash2,
  Wifi,
  WifiOff,
  RefreshCw,
  Power,
  Loader2,
  Lightbulb,
  Plug,
  Search,
  Snowflake,
} from "lucide-react";

const deviceIcons: Record<string, React.ReactNode> = {
  feeder: <Utensils className="h-8 w-8" />,
  fountain: <Droplets className="h-8 w-8" />,
  "litter-box": <Trash2 className="h-8 w-8" />,
  unknown: <Power className="h-8 w-8" />,
};

const deviceColors: Record<string, string> = {
  feeder: "bg-muted text-muted-foreground",
  fountain: "bg-muted text-muted-foreground",
  "litter-box": "bg-muted text-muted-foreground",
  unknown: "bg-muted text-muted-foreground",
};

function DeviceCard({ device }: { device: Device }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const connectMutation = useMutation({
    mutationFn: () => devicesApi.connect(device.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["devices"] });
      toast({
        title: t("device.connectionInitiated"),
        description: t("device.connectionInitiatedDescription", { name: device.name }),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("device.connectionFailed"),
        variant: "destructive",
      });
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: () => devicesApi.disconnect(device.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["devices"] });
      toast({
        title: t("device.disconnected"),
        description: t("device.disconnectedDescription", { name: device.name }),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("device.disconnectionFailed"),
        variant: "destructive",
      });
    },
  });

  const isLoading = connectMutation.isPending || disconnectMutation.isPending;

  return (
    <Card className="border-0 bg-card/85 shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md">
      <CardHeader className="flex flex-row items-center gap-4 pb-4">
        <div
          className={`flex h-16 w-16 items-center justify-center rounded-xl ${
            deviceColors[device.type]
          }`}
        >
          {deviceIcons[device.type]}
        </div>
        <div className="flex-1">
          <CardTitle className="flex items-center gap-2">
            {device.name}
            {device.connected ? (
              <Badge variant="success" className="ml-2">
                <Wifi className="mr-1 h-3 w-3" />
                {t("common.connected")}
              </Badge>
            ) : (
              <Badge variant="secondary" className="ml-2">
                <WifiOff className="mr-1 h-3 w-3" />
                {t("common.disconnected")}
              </Badge>
            )}
          </CardTitle>
          <CardDescription>
            {device.product_name || device.type}
            {device.ip && <span className="ml-2 font-mono text-xs">({device.ip})</span>}
          </CardDescription>
        </div>
      </CardHeader>
      <CardContent>
        <div className="flex gap-2">
          <Link to={`/device/${device.id}`} className="flex-1">
            <Button variant="default" className="w-full">
              {t("common.manage")}
            </Button>
          </Link>
          {device.connected ? (
            <Button
              variant="outline"
              onClick={() => disconnectMutation.mutate()}
              disabled={isLoading}
            >
              {isLoading ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <WifiOff className="h-4 w-4" />
              )}
            </Button>
          ) : (
            <Button variant="outline" onClick={() => connectMutation.mutate()} disabled={isLoading}>
              {isLoading ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Wifi className="h-4 w-4" />
              )}
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function DeviceCardSkeleton() {
  return (
    <Card className="border-0 bg-card/85 shadow-sm">
      <CardHeader className="flex flex-row items-center gap-4 pb-4">
        <Skeleton className="h-16 w-16 rounded-xl" />
        <div className="flex-1 space-y-2">
          <Skeleton className="h-6 w-50" />
          <Skeleton className="h-4 w-37.5" />
        </div>
      </CardHeader>
      <CardContent>
        <div className="flex gap-2">
          <Skeleton className="h-9 flex-1" />
          <Skeleton className="h-9 w-9" />
        </div>
      </CardContent>
    </Card>
  );
}

export function DashboardPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data, isLoading, error } = useQuery({
    queryKey: ["devices"],
    queryFn: devicesApi.list,
    refetchInterval: 10000, // Refresh every 10 seconds
  });

  // Hue lamps query
  const { data: hueLampsData, isLoading: isLoadingHueLamps } = useQuery({
    queryKey: ["hue-lamps"],
    queryFn: hueLampsApi.list,
    refetchInterval: 5000, // Refresh every 5 seconds for lamps
  });

  // Hue lamps stats (to check if disabled)
  const { data: hueLampsStats } = useQuery({
    queryKey: ["hue-lamps-stats"],
    queryFn: hueLampsApi.stats,
    staleTime: 60000, // Cache for 1 minute
  });

  // Check if Hue lamps are disabled (Docker mode)
  const isHueDisabled = hueLampsStats?.disabled === true;

  const { data: zigbeeLampsData, isLoading: isLoadingZigbeeLamps } = useQuery({
    queryKey: ["zigbee-lamps"],
    queryFn: zigbeeLampsApi.list,
    refetchInterval: 5000,
  });

  const { data: zigbeeLampsStats } = useQuery({
    queryKey: ["zigbee-lamps-stats"],
    queryFn: zigbeeLampsApi.stats,
    staleTime: 60000,
  });

  const isZigbeeDisabled = zigbeeLampsStats?.disabled === true;

  // Meross plugs query
  const { data: merossPlugsData, isLoading: isLoadingMeross } = useQuery({
    queryKey: ["meross-plugs"],
    queryFn: merossApi.list,
    refetchInterval: 5000,
  });

  const connectAllMutation = useMutation({
    mutationFn: devicesApi.connectAll,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["devices"] });
      toast({
        title: t("dashboard.globalConnection"),
        description: t("dashboard.globalConnectionDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("device.connectionFailed"),
        variant: "destructive",
      });
    },
  });

  const disconnectAllMutation = useMutation({
    mutationFn: devicesApi.disconnectAll,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["devices"] });
      toast({
        title: t("dashboard.globalDisconnection"),
        description: t("dashboard.globalDisconnectionDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("device.disconnectionFailed"),
        variant: "destructive",
      });
    },
  });

  // Hue lamps scan mutation
  const scanHueLampsMutation = useMutation({
    mutationFn: hueLampsApi.scan,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["hue-lamps"] });
      toast({
        title: t("hueLamps.scanStarted"),
        description: t("hueLamps.scanStartedDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("hueLamps.scanFailed"),
        variant: "destructive",
      });
    },
  });

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center py-12">
        <p className="text-destructive">{t("dashboard.loadingError")}</p>
        <Button
          variant="outline"
          className="mt-4"
          onClick={() => queryClient.invalidateQueries({ queryKey: ["devices"] })}
        >
          <RefreshCw className="mr-2 h-4 w-4" />
          {t("common.retry")}
        </Button>
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <TempoCard />

      <NabaztagTile />

      {!isHueDisabled && (
        <section className="space-y-4">
          <DashboardSectionHeader
            icon={<Lightbulb className="h-5 w-5" />}
            iconClassName="bg-muted text-muted-foreground"
            title={t("hueLamps.title")}
            description={t("hueLamps.subtitle")}
            actions={
              <Button
                variant="outline"
                size="sm"
                className="border-border bg-card/80 hover:bg-card"
                onClick={() => scanHueLampsMutation.mutate()}
                disabled={scanHueLampsMutation.isPending}
              >
                {scanHueLampsMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Search className="mr-2 h-4 w-4" />
                )}
                {t("hueLamps.scan")}
              </Button>
            }
          />

          {isLoadingHueLamps ? (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              <Skeleton className="h-40" />
              <Skeleton className="h-40" />
            </div>
          ) : hueLampsData?.lamps && hueLampsData.lamps.length > 0 ? (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              {hueLampsData.lamps.map((lamp) => (
                <HueLampCard key={lamp.id} lamp={lamp} />
              ))}
            </div>
          ) : (
            <Card className="border-0 bg-card/85 p-8 text-center shadow-sm">
              <Lightbulb className="mx-auto h-12 w-12 text-muted-foreground opacity-50" />
              <p className="mt-4 text-muted-foreground">{t("hueLamps.noLamps")}</p>
              <p className="mt-2 text-sm text-muted-foreground">{t("hueLamps.noLampsHint")}</p>
              <Button
                variant="outline"
                className="mt-4 border-border bg-card/80 hover:bg-card"
                onClick={() => scanHueLampsMutation.mutate()}
                disabled={scanHueLampsMutation.isPending}
              >
                {scanHueLampsMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Search className="mr-2 h-4 w-4" />
                )}
                {t("hueLamps.scanForLamps")}
              </Button>
            </Card>
          )}

          {hueLampsData?.lamps && hueLampsData.lamps.length > 0 && (
            <div className="text-center text-sm text-muted-foreground">
              {t("hueLamps.lampCount", { count: hueLampsData.total })} -{" "}
              {t("hueLamps.connectedCount", { count: hueLampsData.connected })}
            </div>
          )}
        </section>
      )}

      {!isZigbeeDisabled && (
        <section className="space-y-4">
          <DashboardSectionHeader
            icon={<Lightbulb className="h-5 w-5" />}
            iconClassName="bg-amber-100 text-amber-700"
            title={t("zigbeeLamps.title")}
            description={t("zigbeeLamps.subtitle")}
            actions={<ZigbeePairingPanel />}
          />

          {isLoadingZigbeeLamps ? (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              <Skeleton className="h-40" />
              <Skeleton className="h-40" />
            </div>
          ) : zigbeeLampsData?.lamps && zigbeeLampsData.lamps.length > 0 ? (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              {zigbeeLampsData.lamps.map((lamp) => (
                <ZigbeeLampCard key={lamp.id} lamp={lamp} />
              ))}
            </div>
          ) : (
            <Card className="border-0 bg-card/85 p-8 text-center shadow-sm">
              <Lightbulb className="mx-auto h-12 w-12 text-muted-foreground opacity-50" />
              <p className="mt-4 text-muted-foreground">{t("zigbeeLamps.noLamps")}</p>
              <p className="mt-2 text-sm text-muted-foreground">{t("zigbeeLamps.noLampsHint")}</p>
            </Card>
          )}

          {zigbeeLampsData?.lamps && zigbeeLampsData.lamps.length > 0 && (
            <div className="text-center text-sm text-muted-foreground">
              {t("zigbeeLamps.lampCount", { count: zigbeeLampsData.total })} -{" "}
              {t("zigbeeLamps.connectedCount", {
                count: zigbeeLampsData.connected,
              })}
            </div>
          )}
        </section>
      )}

      <section className="space-y-4">
        <DashboardSectionHeader
          icon={<Plug className="h-5 w-5" />}
          iconClassName="bg-muted text-muted-foreground"
          title={t("meross.title")}
          description={t("meross.subtitle")}
        />

        {isLoadingMeross ? (
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            <Skeleton className="h-40" />
            <Skeleton className="h-40" />
          </div>
        ) : merossPlugsData?.devices && merossPlugsData.devices.length > 0 ? (
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {merossPlugsData.devices.map((plug) => (
              <MerossPlugCard key={plug.id} plug={plug} />
            ))}
          </div>
        ) : (
          <Card className="border-0 bg-card/85 p-8 text-center shadow-sm">
            <Plug className="mx-auto h-12 w-12 text-muted-foreground opacity-50" />
            <p className="mt-4 text-muted-foreground">{t("meross.notFound")}</p>
          </Card>
        )}

        {merossPlugsData?.devices && merossPlugsData.devices.length > 0 && (
          <div className="text-center text-sm text-muted-foreground">
            {t("meross.plugCount", { count: merossPlugsData.total })} -{" "}
            {t("meross.onlineCount", {
              count: merossPlugsData.devices.filter((d) => d.isOnline).length,
            })}
          </div>
        )}
      </section>

      <section className="space-y-4">
        <DashboardSectionHeader
          icon={<Power className="h-5 w-5" />}
          iconClassName="bg-muted text-muted-foreground"
          title={t("dashboard.tuyaSection")}
          description={t("dashboard.tuyaSubtitle")}
          actions={
            <>
              <div className="rounded-full border border-border bg-card px-3 py-1 text-sm text-muted-foreground">
                {t("dashboard.deviceCount", { count: data?.total ?? 0 })}
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => queryClient.invalidateQueries({ queryKey: ["devices"] })}
                disabled={isLoading}
                className="flex-1 border-border bg-card/80 hover:bg-card sm:flex-none"
              >
                <RefreshCw className={`mr-2 h-4 w-4 ${isLoading ? "animate-spin" : ""}`} />
                {t("common.refresh")}
              </Button>
              <Button
                variant="default"
                size="sm"
                onClick={() => connectAllMutation.mutate()}
                disabled={connectAllMutation.isPending}
                className="flex-1 bg-primary text-primary-foreground hover:bg-primary/90 sm:flex-none"
              >
                {connectAllMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Wifi className="mr-2 h-4 w-4" />
                )}
                {t("dashboard.connectAll")}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => disconnectAllMutation.mutate()}
                disabled={disconnectAllMutation.isPending}
                className="flex-1 sm:flex-none"
              >
                {disconnectAllMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <WifiOff className="mr-2 h-4 w-4" />
                )}
                {t("dashboard.disconnectAll")}
              </Button>
            </>
          }
        />

        {isLoading ? (
          <div className="grid gap-6 md:grid-cols-2 lg:grid-cols-3">
            <DeviceCardSkeleton />
            <DeviceCardSkeleton />
            <DeviceCardSkeleton />
          </div>
        ) : data?.devices && data.devices.length > 0 ? (
          <div className="grid gap-6 md:grid-cols-2 lg:grid-cols-3">
            {data.devices.map((device) => (
              <DeviceCard key={device.id} device={device} />
            ))}
          </div>
        ) : (
          <Card className="border-0 bg-card/85 p-12 text-center shadow-sm">
            <p className="text-muted-foreground">{t("dashboard.noDevices")}</p>
            <p className="mt-2 text-sm text-muted-foreground">{t("dashboard.noDevicesHint")}</p>
          </Card>
        )}
      </section>

      <section className="space-y-4">
        <DashboardSectionHeader
          icon={<Snowflake className="h-5 w-5" />}
          iconClassName="bg-muted text-muted-foreground"
          title={t("climate.dashboardTitle")}
          description={t("climate.dashboardSubtitle")}
          actions={
            <Button
              variant="ghost"
              size="icon"
              className="rounded-full text-muted-foreground hover:bg-accent hover:text-accent-foreground"
              onClick={() => {
                queryClient.invalidateQueries({ queryKey: ["broadlink", "discover"] });
                queryClient.invalidateQueries({
                  queryKey: ["broadlink", "mitsubishi-codes", "msz-hj5va"],
                });
              }}
            >
              <RefreshCw className="h-4 w-4" />
            </Button>
          }
        />

        <BroadlinkClimateControl defaultModel="msz-hj5va" compact showRefresh={false} />
      </section>
    </div>
  );
}
