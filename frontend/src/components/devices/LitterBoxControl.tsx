import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { litterBoxApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { Slider } from "@/components/ui/slider";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { toast } from "@/hooks/use-toast";
import {
  Trash2,
  Loader2,
  Moon,
  Lock,
  Baby,
  Lightbulb,
  Volume2,
  Home,
  Settings,
  AlertTriangle,
  RefreshCw,
  Play,
} from "lucide-react";

interface LitterBoxControlProps {
  deviceId: string;
}

export function LitterBoxControl({ deviceId }: LitterBoxControlProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [cleanDelayOverride, setCleanDelayOverride] = useState<number[] | null>(null);
  const [sleepStartOverride, setSleepStartOverride] = useState<string | null>(null);
  const [sleepEndOverride, setSleepEndOverride] = useState<string | null>(null);
  const [isResetDialogOpen, setIsResetDialogOpen] = useState(false);

  const { data: statusData, isLoading } = useQuery({
    queryKey: ["litter-box", deviceId, "status"],
    queryFn: () => litterBoxApi.status(deviceId),
    refetchInterval: 15000,
  });

  const cleanMutation = useMutation({
    mutationFn: () => litterBoxApi.clean(deviceId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["litter-box", deviceId] });
      toast({
        title: t("litterBox.cleaningStarted"),
        description: t("litterBox.cleaningCycleStarted"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("litterBox.cleaningFailed"),
        variant: "destructive",
      });
    },
  });

  const settingsMutation = useMutation({
    mutationFn: (settings: Parameters<typeof litterBoxApi.settings>[1]) =>
      litterBoxApi.settings(deviceId, settings),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["litter-box", deviceId] });
      setCleanDelayOverride(null);
      setSleepStartOverride(null);
      setSleepEndOverride(null);
      toast({
        title: t("litterBox.settingsUpdated"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("litterBox.settingsUpdateFailed"),
        variant: "destructive",
      });
    },
  });

  const parsedStatus = statusData?.parsed_status as
    | {
        clean_delay?: {
          seconds?: number;
          formatted?: string;
        };
        sleep_mode?: {
          enabled?: boolean;
          start_time_minutes?: number;
          start_time_formatted?: string;
          end_time_minutes?: number;
          end_time_formatted?: string;
        };
        sensors?: {
          litter_level?: string;
          defecation_frequency?: number;
          defecation_duration?: number;
          fault_alarm?: number;
        };
        system?: {
          state?: string;
          cleaning_in_progress?: boolean;
          maintenance_required?: boolean;
        };
        settings?: {
          lighting?: boolean;
          child_lock?: boolean;
          prompt_sound?: boolean;
          kitten_mode?: boolean;
          automatic_homing?: boolean;
        };
      }
    | undefined;

  // Derive form values: use local override if user touched the control, otherwise server data, otherwise fallback.
  const cleanDelay = cleanDelayOverride ?? [parsedStatus?.clean_delay?.seconds ?? 120];
  const sleepStart =
    sleepStartOverride ?? parsedStatus?.sleep_mode?.start_time_formatted ?? "23:00";
  const sleepEnd = sleepEndOverride ?? parsedStatus?.sleep_mode?.end_time_formatted ?? "07:00";

  if (isLoading) {
    return (
      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <Skeleton className="h-6 w-37.5" />
          </CardHeader>
          <CardContent>
            <Skeleton className="h-75 w-full" />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <Skeleton className="h-6 w-37.5" />
          </CardHeader>
          <CardContent>
            <Skeleton className="h-75 w-full" />
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <Tabs defaultValue="control" className="space-y-4">
      <TabsList>
        <TabsTrigger value="control">
          <Play className="mr-2 h-4 w-4" />
          {t("litterBox.control")}
        </TabsTrigger>
        <TabsTrigger value="settings">
          <Settings className="mr-2 h-4 w-4" />
          {t("litterBox.settings")}
        </TabsTrigger>
      </TabsList>

      <TabsContent value="control" className="space-y-4">
        <div className="grid gap-4 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Trash2 className="h-5 w-5" />
                {t("litterBox.quickActions")}
              </CardTitle>
              <CardDescription>{t("litterBox.quickActionsDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <Button
                onClick={() => cleanMutation.mutate()}
                disabled={cleanMutation.isPending}
                className="w-full"
                size="lg"
              >
                {cleanMutation.isPending ? (
                  <>
                    <Loader2 className="mr-2 h-5 w-5 animate-spin" />
                    {t("litterBox.cleaning")}
                  </>
                ) : (
                  <>
                    <Trash2 className="mr-2 h-5 w-5" />
                    {t("litterBox.startCleaning")}
                  </>
                )}
              </Button>

              <Separator />

              <div className="space-y-3">
                <Label>{t("litterBox.cleanDelayBefore")}</Label>
                <div className="flex items-center gap-4">
                  <Slider
                    value={cleanDelay}
                    onValueChange={setCleanDelayOverride}
                    min={60}
                    max={1800}
                    step={60}
                    className="flex-1"
                  />
                  <Badge variant="outline" className="min-w-15 justify-center">
                    {Math.floor(cleanDelay[0] / 60)} min
                  </Badge>
                </div>
                {parsedStatus?.clean_delay?.seconds && (
                  <p className="text-sm text-muted-foreground">
                    {t("litterBox.currentValue", { value: parsedStatus.clean_delay.formatted })}
                  </p>
                )}
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => settingsMutation.mutate({ clean_delay: cleanDelay[0] })}
                  disabled={settingsMutation.isPending}
                  className="w-full"
                >
                  {settingsMutation.isPending ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : null}
                  {t("litterBox.applyDelay")}
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>{t("litterBox.litterStatus")}</CardTitle>
              <CardDescription>{t("feeder.realtimeInfo")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {parsedStatus?.sensors?.litter_level && (
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <span className="text-sm font-medium">{t("litterBox.litterLevel")}</span>
                    <Badge
                      variant={parsedStatus.sensors.litter_level === "full" ? "success" : "outline"}
                    >
                      {parsedStatus.sensors.litter_level === "full"
                        ? t("litterBox.filled")
                        : parsedStatus.sensors.litter_level === "half"
                          ? t("litterBox.halfFilled")
                          : parsedStatus.sensors.litter_level}
                    </Badge>
                  </div>
                  {parsedStatus.sensors.litter_level === "half" && (
                    <div className="flex items-center gap-2 text-sm text-muted-foreground">
                      <AlertTriangle className="h-4 w-4" />
                      {t("litterBox.fillSoon")}
                    </div>
                  )}
                </div>
              )}

              <Separator />

              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">{t("common.status")}</span>
                <Badge
                  variant={
                    parsedStatus?.system?.state === "cleaning"
                      ? "default"
                      : parsedStatus?.system?.state === "cat_inside"
                        ? "secondary"
                        : "outline"
                  }
                >
                  {parsedStatus?.system?.state === "cleaning"
                    ? t("litterBox.statusCleaning")
                    : parsedStatus?.system?.state === "cat_inside"
                      ? t("litterBox.statusCatInside")
                      : parsedStatus?.system?.state === "clumping"
                        ? t("litterBox.statusClumping")
                        : parsedStatus?.system?.state === "satnd_by"
                          ? t("litterBox.statusStandby")
                          : parsedStatus?.system?.state || t("common.unknown")}
                </Badge>
              </div>

              {parsedStatus?.system?.maintenance_required && (
                <>
                  <Separator />
                  <div className="flex items-center gap-2 text-destructive">
                    <AlertTriangle className="h-4 w-4" />
                    <span className="text-sm">{t("litterBox.maintenanceRequired")}</span>
                  </div>
                </>
              )}

              {parsedStatus?.sensors?.fault_alarm !== 0 &&
                parsedStatus?.sensors?.fault_alarm !== undefined && (
                  <>
                    <Separator />
                    <div className="flex items-center gap-2 text-destructive">
                      <AlertTriangle className="h-4 w-4" />
                      <span className="text-sm">
                        {t("litterBox.faultAlarm", { code: parsedStatus.sensors.fault_alarm })}
                      </span>
                    </div>
                  </>
                )}

              <Separator />

              <Dialog open={isResetDialogOpen} onOpenChange={setIsResetDialogOpen}>
                <DialogTrigger asChild>
                  <Button variant="outline" size="sm" className="w-full">
                    <RefreshCw className="mr-2 h-4 w-4" />
                    {t("litterBox.resetLitterLevel")}
                  </Button>
                </DialogTrigger>
                <DialogContent>
                  <DialogHeader>
                    <DialogTitle>{t("litterBox.resetLitterLevelTitle")}</DialogTitle>
                    <DialogDescription>
                      {t("litterBox.resetLitterLevelDescription")}
                    </DialogDescription>
                  </DialogHeader>
                  <DialogFooter>
                    <Button variant="outline" onClick={() => setIsResetDialogOpen(false)}>
                      {t("common.cancel")}
                    </Button>
                    <Button
                      onClick={() => {
                        settingsMutation.mutate({
                          actions: { reset_sand_level: true },
                        });
                        setIsResetDialogOpen(false);
                      }}
                    >
                      {t("common.confirm")}
                    </Button>
                  </DialogFooter>
                </DialogContent>
              </Dialog>
            </CardContent>
          </Card>
        </div>
      </TabsContent>

      <TabsContent value="settings" className="space-y-4">
        <div className="grid gap-4 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Moon className="h-5 w-5" />
                {t("litterBox.nightMode")}
              </CardTitle>
              <CardDescription>{t("litterBox.nightModeDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex items-center justify-between">
                <Label>{t("litterBox.enableNightMode")}</Label>
                <Switch
                  checked={parsedStatus?.sleep_mode?.enabled ?? false}
                  onCheckedChange={(checked) =>
                    settingsMutation.mutate({
                      sleep_mode: { enabled: checked },
                    })
                  }
                  disabled={settingsMutation.isPending}
                />
              </div>

              <Separator />

              <div className="grid grid-cols-2 gap-4">
                <div className="space-y-2">
                  <Label>{t("litterBox.start")}</Label>
                  <Input
                    type="time"
                    value={sleepStart}
                    onChange={(e) => setSleepStartOverride(e.target.value)}
                  />
                  {parsedStatus?.sleep_mode?.start_time_formatted && (
                    <p className="text-xs text-muted-foreground">
                      {t("common.current")}: {parsedStatus.sleep_mode.start_time_formatted}
                    </p>
                  )}
                </div>
                <div className="space-y-2">
                  <Label>{t("litterBox.end")}</Label>
                  <Input
                    type="time"
                    value={sleepEnd}
                    onChange={(e) => setSleepEndOverride(e.target.value)}
                  />
                  {parsedStatus?.sleep_mode?.end_time_formatted && (
                    <p className="text-xs text-muted-foreground">
                      {t("common.current")}: {parsedStatus.sleep_mode.end_time_formatted}
                    </p>
                  )}
                </div>
              </div>

              <Button
                variant="outline"
                size="sm"
                onClick={() =>
                  settingsMutation.mutate({
                    sleep_mode: {
                      start_time: sleepStart,
                      end_time: sleepEnd,
                    },
                  })
                }
                disabled={settingsMutation.isPending}
                className="w-full"
              >
                {t("litterBox.applySchedule")}
              </Button>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Settings className="h-5 w-5" />
                {t("litterBox.preferences")}
              </CardTitle>
              <CardDescription>{t("litterBox.preferencesDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {/* Child Lock */}
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Lock className="h-4 w-4 text-muted-foreground" />
                  <Label>{t("litterBox.childLock")}</Label>
                </div>
                <Switch
                  checked={parsedStatus?.settings?.child_lock ?? false}
                  onCheckedChange={(checked) =>
                    settingsMutation.mutate({
                      preferences: { child_lock: checked },
                    })
                  }
                  disabled={settingsMutation.isPending}
                />
              </div>

              <Separator />

              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Baby className="h-4 w-4 text-muted-foreground" />
                  <Label>{t("litterBox.kittenMode")}</Label>
                </div>
                <Switch
                  checked={parsedStatus?.settings?.kitten_mode ?? false}
                  onCheckedChange={(checked) =>
                    settingsMutation.mutate({
                      preferences: { kitten_mode: checked },
                    })
                  }
                  disabled={settingsMutation.isPending}
                />
              </div>

              <Separator />

              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Lightbulb className="h-4 w-4 text-muted-foreground" />
                  <Label>{t("litterBox.lighting")}</Label>
                </div>
                <Switch
                  checked={parsedStatus?.settings?.lighting ?? false}
                  onCheckedChange={(checked) =>
                    settingsMutation.mutate({
                      preferences: { lighting: checked },
                    })
                  }
                  disabled={settingsMutation.isPending}
                />
              </div>

              <Separator />

              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Volume2 className="h-4 w-4 text-muted-foreground" />
                  <Label>{t("litterBox.sounds")}</Label>
                </div>
                <Switch
                  checked={parsedStatus?.settings?.prompt_sound ?? false}
                  onCheckedChange={(checked) =>
                    settingsMutation.mutate({
                      preferences: { prompt_sound: checked },
                    })
                  }
                  disabled={settingsMutation.isPending}
                />
              </div>

              <Separator />

              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Home className="h-4 w-4 text-muted-foreground" />
                  <Label>{t("litterBox.automaticHoming")}</Label>
                </div>
                <Switch
                  checked={parsedStatus?.settings?.automatic_homing ?? false}
                  onCheckedChange={(checked) =>
                    settingsMutation.mutate({
                      preferences: { automatic_homing: checked },
                    })
                  }
                  disabled={settingsMutation.isPending}
                />
              </div>
            </CardContent>
          </Card>
        </div>
      </TabsContent>
    </Tabs>
  );
}
