import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { zigbeeLampsApi, type ZigbeeLamp } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toast } from "@/hooks/use-toast";
import {
  Lightbulb,
  LightbulbOff,
  Thermometer,
  Wifi,
  WifiOff,
  Sun,
  Radio,
  Loader2,
  Pencil,
  Palette,
  Sparkles,
} from "lucide-react";

// --- Color utilities: convert between CIE XY and sRGB for the color picker ---

function xyToRgb(x: number, y: number, brightness = 1.0): [number, number, number] {
  const safeY = Math.max(y, 0.00001);
  const z = 1.0 - x - safeY;
  const Y = brightness;
  const X = (Y / safeY) * x;
  const Z = (Y / safeY) * z;
  // Wide-gamut to sRGB matrix
  let r = X * 1.656492 - Y * 0.354851 - Z * 0.255038;
  let g = -X * 0.707196 + Y * 1.655397 + Z * 0.036152;
  let b = X * 0.051713 - Y * 0.121364 + Z * 1.01153;
  // Clamp
  const maxVal = Math.max(r, g, b, 1);
  r = Math.max(0, r / maxVal);
  g = Math.max(0, g / maxVal);
  b = Math.max(0, b / maxVal);
  // Gamma
  r = r <= 0.0031308 ? 12.92 * r : 1.055 * Math.pow(r, 1.0 / 2.4) - 0.055;
  g = g <= 0.0031308 ? 12.92 * g : 1.055 * Math.pow(g, 1.0 / 2.4) - 0.055;
  b = b <= 0.0031308 ? 12.92 * b : 1.055 * Math.pow(b, 1.0 / 2.4) - 0.055;
  return [Math.round(r * 255), Math.round(g * 255), Math.round(b * 255)];
}

function rgbToXy(r: number, g: number, b: number): [number, number] {
  // Normalize
  let rr = r / 255;
  let gg = g / 255;
  let bb = b / 255;
  // Reverse gamma
  rr = rr > 0.04045 ? Math.pow((rr + 0.055) / 1.055, 2.4) : rr / 12.92;
  gg = gg > 0.04045 ? Math.pow((gg + 0.055) / 1.055, 2.4) : gg / 12.92;
  bb = bb > 0.04045 ? Math.pow((bb + 0.055) / 1.055, 2.4) : bb / 12.92;
  // sRGB to XYZ
  const X = rr * 0.664511 + gg * 0.154324 + bb * 0.162028;
  const Y = rr * 0.283881 + gg * 0.668433 + bb * 0.047685;
  const Z = rr * 0.000088 + gg * 0.07231 + bb * 0.986039;
  const sum = X + Y + Z;
  if (sum === 0) return [0.3127, 0.329]; // D65 white
  return [X / sum, Y / sum];
}

function hsvToRgb(h: number, s: number, v: number): [number, number, number] {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let r = 0,
    g = 0,
    b = 0;
  if (h < 60) {
    r = c;
    g = x;
  } else if (h < 120) {
    r = x;
    g = c;
  } else if (h < 180) {
    g = c;
    b = x;
  } else if (h < 240) {
    g = x;
    b = c;
  } else if (h < 300) {
    r = x;
    b = c;
  } else {
    r = c;
    b = x;
  }
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}

/** Render a hue/saturation wheel onto a canvas */
function renderColorWheel(canvas: HTMLCanvasElement) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const w = canvas.width;
  const h = canvas.height;
  const cx = w / 2;
  const cy = h / 2;
  const radius = Math.min(cx, cy);
  const imageData = ctx.createImageData(w, h);
  for (let py = 0; py < h; py++) {
    for (let px = 0; px < w; px++) {
      const dx = px - cx;
      const dy = py - cy;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const idx = (py * w + px) * 4;
      if (dist <= radius) {
        const angle = (Math.atan2(dy, dx) * 180) / Math.PI;
        const hue = (angle + 360) % 360;
        const sat = dist / radius;
        const [r, g, b] = hsvToRgb(hue, sat, 1);
        imageData.data[idx] = r;
        imageData.data[idx + 1] = g;
        imageData.data[idx + 2] = b;
        imageData.data[idx + 3] = 255;
      } else {
        imageData.data[idx + 3] = 0;
      }
    }
  }
  ctx.putImageData(imageData, 0, 0);
}

/** Convert CIE XY (float 0.0–1.0) to canvas position on the color wheel */
function xyToWheelPos(
  colorX: number,
  colorY: number,
  canvasSize: number,
): { px: number; py: number } {
  // CIE XY to sRGB then to HSV angle
  const x = colorX;
  const y = colorY;
  const [r, g, b] = xyToRgb(x, y);
  // RGB to HSV
  const rn = r / 255,
    gn = g / 255,
    bn = b / 255;
  const max = Math.max(rn, gn, bn),
    min = Math.min(rn, gn, bn);
  const delta = max - min;
  let hue = 0;
  if (delta > 0) {
    if (max === rn) hue = 60 * (((gn - bn) / delta) % 6);
    else if (max === gn) hue = 60 * ((bn - rn) / delta + 2);
    else hue = 60 * ((rn - gn) / delta + 4);
    if (hue < 0) hue += 360;
  }
  const sat = max === 0 ? 0 : delta / max;
  const cx = canvasSize / 2;
  const cy = canvasSize / 2;
  const radius = canvasSize / 2;
  const rad = (hue * Math.PI) / 180;
  const dist = sat * radius;
  return { px: cx + dist * Math.cos(rad), py: cy + dist * Math.sin(rad) };
}

/** Convert a click position on the color wheel canvas to CIE XY (float 0.0–1.0) */
function wheelPosToXy(px: number, py: number, canvasSize: number): { x: number; y: number } | null {
  const cx = canvasSize / 2;
  const cy = canvasSize / 2;
  const radius = canvasSize / 2;
  const dx = px - cx;
  const dy = py - cy;
  const dist = Math.sqrt(dx * dx + dy * dy);
  if (dist > radius) return null;
  const angle = (Math.atan2(dy, dx) * 180) / Math.PI;
  const hue = (angle + 360) % 360;
  const sat = dist / radius;
  const [r, g, b] = hsvToRgb(hue, sat, 1);
  const [cieX, cieY] = rgbToXy(r, g, b);
  return { x: cieX, y: cieY };
}

const HUE_EFFECTS = [
  { id: "candle", labelKey: "effectCandle" },
  { id: "fireplace", labelKey: "effectFireplace" },
  { id: "colorloop", labelKey: "effectColorloop" },
  { id: "sunrise", labelKey: "effectSunrise" },
  { id: "sparkle", labelKey: "effectSparkle" },
  { id: "opal", labelKey: "effectOpal" },
  { id: "glisten", labelKey: "effectGlisten" },
  { id: "blink", labelKey: "effectBlink" },
  { id: "breathe", labelKey: "effectBreathe" },
  { id: "okay", labelKey: "effectOkay" },
] as const;

const COLOR_WHEEL_SIZE = 192;

/** Preset color swatches for the compact card view — pre-computed CIE XY (uint16). */
const COLOR_PRESETS: { label: string; css: string; x: number; y: number }[] = (() => {
  const defs: { label: string; css: string; r: number; g: number; b: number }[] = [
    { label: "Red", css: "#ef4444", r: 239, g: 68, b: 68 },
    { label: "Orange", css: "#f97316", r: 249, g: 115, b: 22 },
    { label: "Yellow", css: "#eab308", r: 234, g: 179, b: 8 },
    { label: "Green", css: "#22c55e", r: 34, g: 197, b: 94 },
    { label: "Blue", css: "#3b82f6", r: 59, g: 130, b: 246 },
    { label: "Purple", css: "#a855f7", r: 168, g: 85, b: 247 },
    { label: "Pink", css: "#ec4899", r: 236, g: 72, b: 153 },
    { label: "White", css: "#f5f5f4", r: 245, g: 245, b: 244 },
  ];
  return defs.map((d) => {
    const [cx, cy] = rgbToXy(d.r, d.g, d.b);
    return { label: d.label, css: d.css, x: cx, y: cy };
  });
})();

interface ZigbeeLampControlProps {
  lampId: string;
}

export function ZigbeeLampControl({ lampId }: ZigbeeLampControlProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [renameDialogOpen, setRenameDialogOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const { data, isLoading } = useQuery({
    queryKey: ["zigbee-lamp", lampId],
    queryFn: () => zigbeeLampsApi.status(lampId),
    refetchInterval: 3000,
  });

  const lamp = data?.lamp;
  const [localBrightness, setLocalBrightness] = useState<number[]>([lamp?.state.brightness ?? 0]);
  const [localTemperature, setLocalTemperature] = useState<number[]>([
    lamp?.state.temperature ?? 50,
  ]);
  const targetBrightnessRef = useRef<number | null>(null);
  const targetTemperatureRef = useRef<number | null>(null);
  const powerCooldownRef = useRef<number>(0);
  // Local tab override — tracks which tab the user selected.  Synced from the
  // server's colorMode but NOT overwritten while the user is browsing the color
  // tab (otherwise the 3 s poll would snap it back to "temperature" before the
  // user picks a color).
  const [colorTab, setColorTab] = useState<"temperature" | "color">(
    lamp?.state.colorMode === 2 ? "temperature" : "color",
  );

  useEffect(() => {
    if (lamp) {
      setRenameValue(lamp.name);
    }
  }, [lamp?.name]);

  const invalidateZigbeeQueries = () => {
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps"] });
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps-stats"] });
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamp", lampId] });
  };

  const renameMutation = useMutation({
    mutationFn: (name: string) => zigbeeLampsApi.rename(lampId, name),
    onSuccess: () => {
      invalidateZigbeeQueries();
      setRenameDialogOpen(false);
      toast({
        title: t("zigbeeLamps.renameSuccess"),
        description: t("zigbeeLamps.renameSuccessDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.renameFailed"),
        variant: "destructive",
      });
    },
  });

  const powerMutation = useMutation({
    mutationFn: (enabled: boolean) => zigbeeLampsApi.power(lampId, enabled),
    onSuccess: (_, enabled) => {
      invalidateZigbeeQueries();
      toast({
        title: enabled ? t("zigbeeLamps.lampOn") : t("zigbeeLamps.lampOff"),
        description: t("zigbeeLamps.powerChanged"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.powerFailed"),
        variant: "destructive",
      });
    },
  });

  const brightnessMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.brightness(lampId, value),
    onSuccess: () => {
      invalidateZigbeeQueries();
    },
    onError: (error) => {
      targetBrightnessRef.current = null;
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.brightnessFailed"),
        variant: "destructive",
      });
    },
  });

  const temperatureMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.temperature(lampId, value),
    onSuccess: () => {
      setColorTab("temperature");
      invalidateZigbeeQueries();
    },
    onError: (error) => {
      targetTemperatureRef.current = null;
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.temperatureFailed"),
        variant: "destructive",
      });
    },
  });

  const colorMutation = useMutation({
    mutationFn: ({ x, y }: { x: number; y: number }) => zigbeeLampsApi.color(lampId, x, y),
    onSuccess: () => {
      setColorTab("color");
      invalidateZigbeeQueries();
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.colorFailed"),
        variant: "destructive",
      });
    },
  });

  const effectMutation = useMutation({
    mutationFn: (effect: string) => zigbeeLampsApi.effect(lampId, effect),
    onSuccess: () => {
      invalidateZigbeeQueries();
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.effectFailed"),
        variant: "destructive",
      });
    },
  });

  const colorWheelRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (colorWheelRef.current) {
      renderColorWheel(colorWheelRef.current);
    }
  });

  const handleColorWheelClick = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const canvas = colorWheelRef.current;
      if (!canvas) return;
      const rect = canvas.getBoundingClientRect();
      const scaleX = canvas.width / rect.width;
      const scaleY = canvas.height / rect.height;
      const px = (e.clientX - rect.left) * scaleX;
      const py = (e.clientY - rect.top) * scaleY;
      const result = wheelPosToXy(px, py, COLOR_WHEEL_SIZE);
      if (result) {
        colorMutation.mutate(result);
      }
    },
    [colorMutation],
  );

  useEffect(() => {
    if (lamp?.state.brightness === undefined) return;
    const now = Date.now();
    if (now - powerCooldownRef.current < 1000) return;
    const serverValue = lamp.state.brightness;
    const target = targetBrightnessRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetBrightnessRef.current = null;
        setLocalBrightness([serverValue]);
      }
    } else {
      setLocalBrightness([serverValue]);
    }
  }, [lamp?.state.brightness]);

  useEffect(() => {
    if (lamp?.state.temperature === undefined || lamp?.state.temperature === null) return;
    const serverValue = lamp.state.temperature;
    const target = targetTemperatureRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetTemperatureRef.current = null;
        setLocalTemperature([serverValue]);
      }
    } else {
      setLocalTemperature([serverValue]);
    }
  }, [lamp?.state.temperature]);

  if (isLoading) {
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

  if (!lamp) {
    return (
      <Card>
        <CardContent className="py-8 text-center">
          <LightbulbOff className="mx-auto h-12 w-12 text-muted-foreground" />
          <p className="mt-4 text-muted-foreground">{t("zigbeeLamps.notFound")}</p>
        </CardContent>
      </Card>
    );
  }

  const isConnected = lamp.reachable;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-center gap-3">
              <div
                className={`flex h-12 w-12 items-center justify-center rounded-xl ${
                  lamp.state.isOn
                    ? "bg-amber-100 text-amber-700 dark:bg-amber-950 dark:text-amber-300"
                    : "bg-muted text-muted-foreground"
                }`}
              >
                {lamp.state.isOn ? (
                  <Lightbulb className="h-6 w-6" />
                ) : (
                  <LightbulbOff className="h-6 w-6" />
                )}
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <CardTitle>{lamp.name}</CardTitle>
                  <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
                    <DialogTrigger asChild>
                      <Button variant="ghost" size="icon" className="h-8 w-8">
                        <Pencil className="h-4 w-4" />
                      </Button>
                    </DialogTrigger>
                    <DialogContent>
                      <DialogHeader>
                        <DialogTitle>{t("zigbeeLamps.renameTitle")}</DialogTitle>
                        <DialogDescription>
                          {t("zigbeeLamps.renameDescription", { name: lamp.name })}
                        </DialogDescription>
                      </DialogHeader>
                      <Input
                        value={renameValue}
                        onChange={(event) => setRenameValue(event.target.value)}
                      />
                      <DialogFooter>
                        <Button variant="outline" onClick={() => setRenameDialogOpen(false)}>
                          {t("common.cancel")}
                        </Button>
                        <Button
                          onClick={() => renameMutation.mutate(renameValue)}
                          disabled={renameMutation.isPending || renameValue.trim().length === 0}
                        >
                          {renameMutation.isPending ? (
                            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                          ) : null}
                          {t("zigbeeLamps.renameConfirm")}
                        </Button>
                      </DialogFooter>
                    </DialogContent>
                  </Dialog>
                </div>
                <CardDescription>
                  {lamp.model || t("zigbeeLamps.unknownModel")} - {lamp.manufacturer}
                </CardDescription>
              </div>
            </div>
            <Switch
              checked={lamp.state.isOn}
              onCheckedChange={(checked) => {
                powerCooldownRef.current = Date.now();
                powerMutation.mutate(checked);
              }}
              disabled={!isConnected || powerMutation.isPending}
            />
          </div>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="flex flex-wrap gap-2">
            {lamp.reachable ? (
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
            <Badge variant="outline">
              <Radio className="mr-1 h-3 w-3" />
              {lamp.interviewCompleted
                ? t("zigbeeLamps.interviewComplete")
                : t("zigbeeLamps.interviewPending")}
            </Badge>
          </div>

          <div className="space-y-3">
            <div>
              <Label>{t("zigbeeLamps.brightness")}</Label>
              <p className="text-sm text-muted-foreground">
                {t("zigbeeLamps.brightnessDescription")}
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Sun className="h-4 w-4 text-muted-foreground" />
              <Slider
                value={localBrightness}
                onValueChange={setLocalBrightness}
                onValueCommit={(value) => {
                  const newValue = value[0];
                  targetBrightnessRef.current = newValue;
                  setLocalBrightness([newValue]);
                  brightnessMutation.mutate(newValue);
                }}
                min={0}
                max={100}
                step={1}
                disabled={!isConnected || !lamp.state.isOn || brightnessMutation.isPending}
              />
              <span className="w-10 text-right text-sm text-muted-foreground">
                {localBrightness[0]}%
              </span>
            </div>
          </div>

          {/* --- Color mode tabs (Temperature / Color) --- */}
          {lamp.supportsColor && lamp.state.temperature !== null ? (
            <Tabs
              value={colorTab}
              onValueChange={(tab) => {
                const next = tab as "temperature" | "color";
                setColorTab(next);
                if (next === "temperature") {
                  temperatureMutation.mutate(localTemperature[0]);
                }
              }}
            >
              <TabsList className="w-full">
                <TabsTrigger value="temperature" className="flex-1 gap-1.5">
                  <Thermometer className="h-3.5 w-3.5" />
                  {t("zigbeeLamps.modeTemperature")}
                </TabsTrigger>
                <TabsTrigger value="color" className="flex-1 gap-1.5">
                  <Palette className="h-3.5 w-3.5" />
                  {t("zigbeeLamps.modeColor")}
                </TabsTrigger>
              </TabsList>

              <TabsContent value="temperature">
                <div className="space-y-3">
                  <p className="text-sm text-muted-foreground">
                    {t("zigbeeLamps.temperatureDescription")}
                  </p>
                  <div className="flex items-center gap-3">
                    <Thermometer className="h-4 w-4 text-muted-foreground" />
                    <Slider
                      value={localTemperature}
                      onValueChange={setLocalTemperature}
                      onValueCommit={(value) => {
                        const newValue = value[0];
                        targetTemperatureRef.current = newValue;
                        setLocalTemperature([newValue]);
                        temperatureMutation.mutate(newValue);
                      }}
                      min={lamp.state.temperatureMin ?? 0}
                      max={lamp.state.temperatureMax ?? 100}
                      step={1}
                      disabled={!isConnected || !lamp.state.isOn || temperatureMutation.isPending}
                    />
                    <span className="w-10 text-right text-sm text-muted-foreground">
                      {localTemperature[0]}%
                    </span>
                  </div>
                </div>
              </TabsContent>

              <TabsContent value="color">
                <div className="space-y-4">
                  <div className="space-y-3">
                    <p className="text-sm text-muted-foreground">
                      {t("zigbeeLamps.colorDescription")}
                    </p>
                    <div className="flex items-center gap-4">
                      <Palette className="h-4 w-4 shrink-0 text-muted-foreground" />
                      <div className="relative">
                        <canvas
                          ref={colorWheelRef}
                          width={COLOR_WHEEL_SIZE}
                          height={COLOR_WHEEL_SIZE}
                          className="h-48 w-48 cursor-pointer rounded-full"
                          onClick={handleColorWheelClick}
                        />
                        {lamp.state.colorX !== null &&
                          lamp.state.colorY !== null &&
                          (() => {
                            const pos = xyToWheelPos(
                              lamp.state.colorX,
                              lamp.state.colorY,
                              COLOR_WHEEL_SIZE,
                            );
                            const displayScale = 192 / COLOR_WHEEL_SIZE;
                            return (
                              <div
                                className="pointer-events-none absolute h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-md"
                                style={{ left: pos.px * displayScale, top: pos.py * displayScale }}
                              />
                            );
                          })()}
                      </div>
                      {colorMutation.isPending && (
                        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                      )}
                    </div>
                  </div>

                  <div className="space-y-3">
                    <div>
                      <Label>{t("zigbeeLamps.effects")}</Label>
                      <p className="text-sm text-muted-foreground">
                        {t("zigbeeLamps.effectsDescription")}
                      </p>
                    </div>
                    <div className="flex items-start gap-3">
                      <Sparkles className="mt-1 h-4 w-4 shrink-0 text-muted-foreground" />
                      <div className="flex flex-wrap gap-2">
                        {HUE_EFFECTS.map((eff) => (
                          <Button
                            key={eff.id}
                            variant="outline"
                            size="sm"
                            disabled={!isConnected || !lamp.state.isOn || effectMutation.isPending}
                            onClick={() => effectMutation.mutate(eff.id)}
                          >
                            {t(`zigbeeLamps.${eff.labelKey}`)}
                          </Button>
                        ))}
                        <Button
                          variant="secondary"
                          size="sm"
                          disabled={!isConnected || !lamp.state.isOn || effectMutation.isPending}
                          onClick={() => effectMutation.mutate("stop_hue_effect")}
                        >
                          {t("zigbeeLamps.effectStopEffect")}
                        </Button>
                      </div>
                    </div>
                  </div>
                </div>
              </TabsContent>
            </Tabs>
          ) : lamp.state.temperature !== null ? (
            /* Temperature-only lamp (no color support) */
            <div className="space-y-3">
              <div>
                <Label>{t("zigbeeLamps.temperature")}</Label>
                <p className="text-sm text-muted-foreground">
                  {t("zigbeeLamps.temperatureDescription")}
                </p>
              </div>
              <div className="flex items-center gap-3">
                <Thermometer className="h-4 w-4 text-muted-foreground" />
                <Slider
                  value={localTemperature}
                  onValueChange={setLocalTemperature}
                  onValueCommit={(value) => {
                    const newValue = value[0];
                    targetTemperatureRef.current = newValue;
                    setLocalTemperature([newValue]);
                    temperatureMutation.mutate(newValue);
                  }}
                  min={lamp.state.temperatureMin ?? 0}
                  max={lamp.state.temperatureMax ?? 100}
                  step={1}
                  disabled={!isConnected || !lamp.state.isOn || temperatureMutation.isPending}
                />
                <span className="w-10 text-right text-sm text-muted-foreground">
                  {localTemperature[0]}%
                </span>
              </div>
            </div>
          ) : lamp.supportsColor ? (
            /* Color-only lamp (no temperature support) */
            <>
              <div className="space-y-3">
                <div>
                  <Label>{t("zigbeeLamps.color")}</Label>
                  <p className="text-sm text-muted-foreground">
                    {t("zigbeeLamps.colorDescription")}
                  </p>
                </div>
                <div className="flex items-center gap-4">
                  <Palette className="h-4 w-4 shrink-0 text-muted-foreground" />
                  <div className="relative">
                    <canvas
                      ref={colorWheelRef}
                      width={COLOR_WHEEL_SIZE}
                      height={COLOR_WHEEL_SIZE}
                      className="h-48 w-48 cursor-pointer rounded-full"
                      onClick={handleColorWheelClick}
                    />
                    {lamp.state.colorX !== null &&
                      lamp.state.colorY !== null &&
                      (() => {
                        const pos = xyToWheelPos(
                          lamp.state.colorX,
                          lamp.state.colorY,
                          COLOR_WHEEL_SIZE,
                        );
                        const displayScale = 192 / COLOR_WHEEL_SIZE;
                        return (
                          <div
                            className="pointer-events-none absolute h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-md"
                            style={{ left: pos.px * displayScale, top: pos.py * displayScale }}
                          />
                        );
                      })()}
                  </div>
                  {colorMutation.isPending && (
                    <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                  )}
                </div>
              </div>
              <div className="space-y-3">
                <div>
                  <Label>{t("zigbeeLamps.effects")}</Label>
                  <p className="text-sm text-muted-foreground">
                    {t("zigbeeLamps.effectsDescription")}
                  </p>
                </div>
                <div className="flex items-start gap-3">
                  <Sparkles className="mt-1 h-4 w-4 shrink-0 text-muted-foreground" />
                  <div className="flex flex-wrap gap-2">
                    {HUE_EFFECTS.map((eff) => (
                      <Button
                        key={eff.id}
                        variant="outline"
                        size="sm"
                        disabled={!isConnected || !lamp.state.isOn || effectMutation.isPending}
                        onClick={() => effectMutation.mutate(eff.id)}
                      >
                        {t(`zigbeeLamps.${eff.labelKey}`)}
                      </Button>
                    ))}
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={!isConnected || !lamp.state.isOn || effectMutation.isPending}
                      onClick={() => effectMutation.mutate("stop_hue_effect")}
                    >
                      {t("zigbeeLamps.effectStopEffect")}
                    </Button>
                  </div>
                </div>
              </div>
            </>
          ) : null}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("zigbeeLamps.deviceInfo")}</CardTitle>
          <CardDescription>{t("zigbeeLamps.deviceInfoDescription")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.friendlyName")}</span>
            <span className="font-mono text-right">{lamp.friendlyName}</span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.address")}</span>
            <span className="font-mono text-right">{lamp.address}</span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.linkQuality")}</span>
            <span>{lamp.linkQuality ?? "-"}</span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.lastSeen")}</span>
            <span>{lamp.lastSeen ? new Date(lamp.lastSeen).toLocaleString() : "-"}</span>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

interface ZigbeeLampCardProps {
  lamp: ZigbeeLamp;
}

export function ZigbeeLampCard({ lamp }: ZigbeeLampCardProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [localBrightness, setLocalBrightness] = useState([lamp.state.brightness]);
  const [localTemperature, setLocalTemperature] = useState([lamp.state.temperature ?? 50]);
  const targetBrightnessRef = useRef<number | null>(null);
  const targetTemperatureRef = useRef<number | null>(null);
  const powerCooldownRef = useRef<number>(0);
  const [colorTab, setColorTab] = useState<"temperature" | "color">(
    lamp.state.colorMode === 2 ? "temperature" : "color",
  );

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps"] });
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps-stats"] });
  };

  const powerMutation = useMutation({
    mutationFn: (enabled: boolean) => zigbeeLampsApi.power(lamp.id, enabled),
    onSuccess: invalidate,
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.powerFailed"),
        variant: "destructive",
      });
    },
  });

  const brightnessMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.brightness(lamp.id, value),
    onSuccess: invalidate,
  });

  const temperatureMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.temperature(lamp.id, value),
    onSuccess: () => {
      setColorTab("temperature");
      invalidate();
    },
  });

  const colorMutation = useMutation({
    mutationFn: ({ x, y }: { x: number; y: number }) => zigbeeLampsApi.color(lamp.id, x, y),
    onSuccess: () => {
      setColorTab("color");
      invalidate();
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.colorFailed"),
        variant: "destructive",
      });
    },
  });

  useEffect(() => {
    const now = Date.now();
    if (now - powerCooldownRef.current < 1000) return;
    const serverValue = lamp.state.brightness;
    const target = targetBrightnessRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetBrightnessRef.current = null;
        setLocalBrightness([serverValue]);
      }
    } else {
      setLocalBrightness([serverValue]);
    }
  }, [lamp.state.brightness]);

  useEffect(() => {
    if (lamp.state.temperature === null) return;
    const serverValue = lamp.state.temperature;
    const target = targetTemperatureRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetTemperatureRef.current = null;
        setLocalTemperature([serverValue]);
      }
    } else {
      setLocalTemperature([serverValue]);
    }
  }, [lamp.state.temperature]);

  return (
    <Card className="transition-shadow hover:shadow-lg">
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div
              className={`flex h-10 w-10 items-center justify-center rounded-lg ${
                lamp.state.isOn
                  ? "bg-amber-100 text-amber-700 dark:bg-amber-950 dark:text-amber-300"
                  : "bg-muted text-muted-foreground"
              }`}
            >
              {lamp.state.isOn ? (
                <Lightbulb className="h-5 w-5" />
              ) : (
                <LightbulbOff className="h-5 w-5" />
              )}
            </div>
            <div>
              <Link to={`/zigbee-lamp/${lamp.id}`}>
                <CardTitle className="cursor-pointer text-base hover:underline">
                  {lamp.name}
                </CardTitle>
              </Link>
              <CardDescription className="text-xs">
                {lamp.model || t("zigbeeLamps.unknownModel")}
              </CardDescription>
            </div>
          </div>
          <Switch
            checked={lamp.state.isOn}
            onCheckedChange={(checked) => {
              powerCooldownRef.current = Date.now();
              powerMutation.mutate(checked);
            }}
            disabled={!lamp.reachable || powerMutation.isPending}
          />
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex items-center justify-between text-xs">
          {lamp.reachable ? (
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
          <span className="text-muted-foreground">{localBrightness[0]}%</span>
        </div>
        <div className="flex items-center gap-2">
          <Sun className="h-3 w-3 shrink-0 text-muted-foreground" />
          <Slider
            value={localBrightness}
            onValueChange={setLocalBrightness}
            onValueCommit={(value) => {
              const newValue = value[0];
              targetBrightnessRef.current = newValue;
              setLocalBrightness([newValue]);
              brightnessMutation.mutate(newValue);
            }}
            min={0}
            max={100}
            step={1}
            disabled={!lamp.reachable || !lamp.state.isOn}
          />
        </div>
        {lamp.supportsColor && lamp.state.temperature !== null ? (
          <Tabs
            value={colorTab}
            onValueChange={(tab) => {
              const next = tab as "temperature" | "color";
              setColorTab(next);
              if (next === "temperature") {
                temperatureMutation.mutate(localTemperature[0]);
              }
            }}
          >
            <TabsList className="w-full">
              <TabsTrigger value="temperature" className="flex-1 gap-1">
                <Thermometer className="h-3 w-3" />
                <span className="text-xs">{t("zigbeeLamps.modeTemperature")}</span>
              </TabsTrigger>
              <TabsTrigger value="color" className="flex-1 gap-1">
                <Palette className="h-3 w-3" />
                <span className="text-xs">{t("zigbeeLamps.modeColor")}</span>
              </TabsTrigger>
            </TabsList>

            <TabsContent value="temperature">
              <div className="flex items-center gap-2">
                <Thermometer className="h-3 w-3 shrink-0 text-muted-foreground" />
                <Slider
                  value={localTemperature}
                  onValueChange={setLocalTemperature}
                  onValueCommit={(value) => {
                    const newValue = value[0];
                    targetTemperatureRef.current = newValue;
                    setLocalTemperature([newValue]);
                    temperatureMutation.mutate(newValue);
                  }}
                  min={lamp.state.temperatureMin ?? 0}
                  max={lamp.state.temperatureMax ?? 100}
                  step={1}
                  disabled={!lamp.reachable || !lamp.state.isOn}
                />
              </div>
            </TabsContent>

            <TabsContent value="color">
              <div className="flex items-center gap-2">
                <Palette className="h-3 w-3 shrink-0 text-muted-foreground" />
                <div className="flex flex-wrap gap-1.5">
                  {COLOR_PRESETS.map((preset) => (
                    <button
                      key={preset.label}
                      title={preset.label}
                      className="h-5 w-5 rounded-full border border-border transition-transform hover:scale-125 disabled:opacity-40"
                      style={{ backgroundColor: preset.css }}
                      disabled={!lamp.reachable || !lamp.state.isOn || colorMutation.isPending}
                      onClick={() => colorMutation.mutate({ x: preset.x, y: preset.y })}
                    />
                  ))}
                </div>
              </div>
            </TabsContent>
          </Tabs>
        ) : lamp.state.temperature !== null ? (
          <div className="flex items-center gap-2">
            <Thermometer className="h-3 w-3 shrink-0 text-muted-foreground" />
            <Slider
              value={localTemperature}
              onValueChange={setLocalTemperature}
              onValueCommit={(value) => {
                const newValue = value[0];
                targetTemperatureRef.current = newValue;
                setLocalTemperature([newValue]);
                temperatureMutation.mutate(newValue);
              }}
              min={lamp.state.temperatureMin ?? 0}
              max={lamp.state.temperatureMax ?? 100}
              step={1}
              disabled={!lamp.reachable || !lamp.state.isOn}
            />
          </div>
        ) : lamp.supportsColor ? (
          <div className="flex items-center gap-2">
            <Palette className="h-3 w-3 shrink-0 text-muted-foreground" />
            <div className="flex flex-wrap gap-1.5">
              {COLOR_PRESETS.map((preset) => (
                <button
                  key={preset.label}
                  title={preset.label}
                  className="h-5 w-5 rounded-full border border-border transition-transform hover:scale-125 disabled:opacity-40"
                  style={{ backgroundColor: preset.css }}
                  disabled={!lamp.reachable || !lamp.state.isOn || colorMutation.isPending}
                  onClick={() => colorMutation.mutate({ x: preset.x, y: preset.y })}
                />
              ))}
            </div>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

export function ZigbeePairingPanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data } = useQuery({
    queryKey: ["zigbee-pairing-status"],
    queryFn: zigbeeLampsApi.pairingStatus,
    refetchInterval: 1000,
  });

  const pairing = data?.pairing;

  const startPairingMutation = useMutation({
    mutationFn: zigbeeLampsApi.startPairing,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["zigbee-pairing-status"] });
      toast({
        title: t("zigbeeLamps.pairingStarted"),
        description: t("zigbeeLamps.pairingStartedDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.pairingFailed"),
        variant: "destructive",
      });
    },
  });

  const stopPairingMutation = useMutation({
    mutationFn: zigbeeLampsApi.stopPairing,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["zigbee-pairing-status"] });
      toast({
        title: t("zigbeeLamps.pairingStopped"),
        description: t("zigbeeLamps.pairingStoppedDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.pairingFailed"),
        variant: "destructive",
      });
    },
  });

  const touchlinkScanMutation = useMutation({
    mutationFn: zigbeeLampsApi.touchlinkScan,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["zigbee-pairing-status"] });
      toast({
        title: t("zigbeeLamps.touchlinkStarted"),
        description: t("zigbeeLamps.touchlinkStartedDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.touchlinkFailed"),
        variant: "destructive",
      });
    },
  });

  return (
    <div className="flex flex-wrap items-center gap-2">
      {pairing?.active ? (
        <Badge variant="success">
          {t("zigbeeLamps.pairingActive", { count: pairing.remainingSeconds })}
        </Badge>
      ) : (
        <Badge variant="secondary">{t("zigbeeLamps.pairingInactive")}</Badge>
      )}
      {pairing?.message && <span className="text-sm text-muted-foreground">{pairing.message}</span>}
      <Button
        variant="outline"
        size="sm"
        onClick={() => startPairingMutation.mutate()}
        disabled={startPairingMutation.isPending || pairing?.active === true}
      >
        {startPairingMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
        {t("zigbeeLamps.startPairing")}
      </Button>
      <Button
        variant="secondary"
        size="sm"
        onClick={() => stopPairingMutation.mutate()}
        disabled={stopPairingMutation.isPending || pairing?.active !== true}
      >
        {stopPairingMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
        {t("zigbeeLamps.stopPairing")}
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() => touchlinkScanMutation.mutate()}
        disabled={touchlinkScanMutation.isPending}
      >
        {touchlinkScanMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
        {t("zigbeeLamps.touchlinkScan")}
      </Button>
    </div>
  );
}
