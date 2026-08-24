import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import {
  broadlinkApi,
  irApi,
  merossApi,
  zigbeeLampsApi,
  type BroadlinkCode,
  type BroadlinkDevice,
  type IrAction,
  type IrBinding,
  type IrSwitchState,
  type MerossPlug,
  type ZigbeeLamp,
} from "@/lib/api";
import { remoteKeyLabel } from "@/components/devices/IrRemoteMap";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { toast } from "@/hooks/use-toast";
import {
  ChevronDown,
  ChevronUp,
  FlaskConical,
  Loader2,
  Plus,
  Radio,
  Save,
  Trash2,
} from "lucide-react";

/** Devices offered in the action pickers (real devices, not free text). */
export interface IrActionSources {
  lamps: ZigbeeLamp[];
  plugs: MerossPlug[];
  hosts: BroadlinkDevice[];
  codes: BroadlinkCode[];
}

/** Reuses the query keys of the other sections so the caches are shared. */
export function useIrActionSources(): IrActionSources {
  const zigbeeQuery = useQuery({
    queryKey: ["zigbee-lamps"],
    queryFn: zigbeeLampsApi.list,
    staleTime: 30_000,
  });

  const merossQuery = useQuery({
    queryKey: ["meross-plugs"],
    queryFn: merossApi.list,
    staleTime: 30_000,
  });

  const broadlinkDevicesQuery = useQuery({
    queryKey: ["broadlink", "discover", "ir-config"],
    queryFn: () => broadlinkApi.discover(),
    staleTime: 60_000,
  });

  const broadlinkCodesQuery = useQuery({
    queryKey: ["broadlink", "codes"],
    queryFn: broadlinkApi.listCodes,
    staleTime: 60_000,
  });

  return {
    lamps: zigbeeQuery.data?.lamps ?? [],
    plugs: merossQuery.data?.devices ?? [],
    hosts: broadlinkDevicesQuery.data?.devices ?? [],
    codes: broadlinkCodesQuery.data?.codes ?? [],
  };
}

/** Human-readable one-liner for an action, with real device names. */
export function summarizeIrAction(
  action: IrAction,
  sources: IrActionSources,
  t: TFunction,
): string {
  switch (action.action) {
    case "nabaztag":
      return t("remote.summary.nabaztag", { command: action.command });
    case "zigbee_power": {
      const lamp = sources.lamps.find((l) => l.id === action.lamp)?.name ?? action.lamp;
      return t(`remote.summary.zigbee_${action.state}`, { lamp });
    }
    case "zigbee_brightness": {
      const lamp = sources.lamps.find((l) => l.id === action.lamp)?.name ?? action.lamp;
      return t("remote.summary.zigbeeBrightness", { lamp, brightness: action.brightness });
    }
    case "broadlink_code": {
      const host = sources.hosts.find((h) => h.host === action.host)?.name ?? action.host;
      const code = sources.codes.find((c) => c.id === action.code_id)?.name ?? action.code_id;
      return t("remote.summary.broadlink", { host, code });
    }
    case "meross_power": {
      const device = sources.plugs.find((p) => p.id === action.device)?.name ?? action.device;
      return t(`remote.summary.meross_${action.state}`, { device });
    }
    case "climate_toggle": {
      const host = sources.hosts.find((h) => h.host === action.host)?.name ?? action.host;
      const settings = parseClimateCommand(action.on_command);
      return t("remote.summary.climate", {
        host,
        settings: settings
          ? `${settings.mode} ${settings.temperature}° fan ${settings.fan} vane ${settings.vane}`
          : action.on_command,
      });
    }
  }
}

const ACTION_TYPES = [
  "nabaztag",
  "zigbee_power",
  "zigbee_brightness",
  "broadlink_code",
  "meross_power",
  "climate_toggle",
] as const;

type IrActionType = (typeof ACTION_TYPES)[number];

const NABAZTAG_PRESETS = ["chor taichi", "dance 1", "ping", "stop"];

// Structured Mitsubishi command, mirroring the backend grammar
// (backend/src/mitsubishi_ir.rs): state-<mode>-<temp>-fan-<fan>-vane-<vane>.
const CLIMATE_MODES = ["auto", "cool", "dry", "heat", "fan"] as const;
const CLIMATE_FANS = ["auto", "1", "2", "3", "4", "silent"] as const;
const CLIMATE_VANES = ["auto", "highest", "high", "middle", "low", "lowest", "swing"] as const;

interface ClimateSettingsForm {
  mode: string;
  temperature: number;
  fan: string;
  vane: string;
}

function buildClimateCommand(settings: ClimateSettingsForm): string {
  return `state-${settings.mode}-${settings.temperature}-fan-${settings.fan}-vane-${settings.vane}`;
}

/** Returns null for advanced commands (econo/timer/…) — edited as raw text. */
function parseClimateCommand(command: string): ClimateSettingsForm | null {
  const match = command.match(/^state-([a-z]+)-(\d+)-fan-([a-z0-9]+)-vane-([a-z]+)$/);
  if (!match) return null;
  return {
    mode: match[1],
    temperature: Number.parseInt(match[2], 10),
    fan: match[3],
    vane: match[4],
  };
}

function makeDefaultAction(type: IrActionType, sources: IrActionSources): IrAction {
  switch (type) {
    case "nabaztag":
      return { action: "nabaztag", command: "" };
    case "zigbee_power":
      return { action: "zigbee_power", lamp: sources.lamps[0]?.id ?? "", state: "toggle" };
    case "zigbee_brightness":
      return {
        action: "zigbee_brightness",
        lamp: sources.lamps[0]?.id ?? "",
        brightness: 127,
      };
    case "broadlink_code":
      return {
        action: "broadlink_code",
        host: sources.hosts[0]?.host ?? "",
        code_id: sources.codes[0]?.id ?? "",
      };
    case "meross_power":
      return { action: "meross_power", device: sources.plugs[0]?.id ?? "", state: "toggle" };
    case "climate_toggle":
      return {
        action: "climate_toggle",
        host: sources.hosts[0]?.host ?? "",
        on_command: buildClimateCommand({
          mode: "cool",
          temperature: 16,
          fan: "4",
          vane: "swing",
        }),
      };
  }
}

function isActionComplete(action: IrAction): boolean {
  switch (action.action) {
    case "nabaztag":
      return action.command.trim().length > 0;
    case "zigbee_power":
    case "zigbee_brightness":
      return action.lamp.length > 0;
    case "broadlink_code":
      return action.host.length > 0 && action.code_id.length > 0;
    case "meross_power":
      return action.device.length > 0;
    case "climate_toggle":
      return action.host.length > 0 && action.on_command.length > 0;
  }
}

function SwitchStateSelect({
  value,
  onChange,
}: {
  value: IrSwitchState;
  onChange: (state: IrSwitchState) => void;
}) {
  const { t } = useTranslation();

  return (
    <Select value={value} onValueChange={(next: IrSwitchState) => onChange(next)}>
      <SelectTrigger>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="toggle">{t("remote.fields.toggle")}</SelectItem>
        <SelectItem value="on">{t("remote.fields.turnOn")}</SelectItem>
        <SelectItem value="off">{t("remote.fields.turnOff")}</SelectItem>
      </SelectContent>
    </Select>
  );
}

function ZigbeeLampSelect({
  value,
  lamps,
  onChange,
}: {
  value: string;
  lamps: ZigbeeLamp[];
  onChange: (lampId: string) => void;
}) {
  const { t } = useTranslation();
  // A saved binding can reference a lamp that is no longer discovered:
  // keep it selectable instead of silently blanking the field.
  const knownIds = lamps.map((lamp) => lamp.id);

  return (
    <Select value={value || undefined} onValueChange={onChange}>
      <SelectTrigger>
        <SelectValue placeholder={t("remote.fields.lampPlaceholder")} />
      </SelectTrigger>
      <SelectContent>
        {lamps.map((lamp) => (
          <SelectItem key={lamp.id} value={lamp.id}>
            {lamp.name}
          </SelectItem>
        ))}
        {value && !knownIds.includes(value) && <SelectItem value={value}>{value}</SelectItem>}
      </SelectContent>
    </Select>
  );
}

interface ActionRowProps {
  action: IrAction;
  index: number;
  count: number;
  sources: IrActionSources;
  onChange: (action: IrAction) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
}

function ActionRow({ action, index, count, sources, onChange, onMove, onRemove }: ActionRowProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-3 rounded-lg border p-3">
      <div className="flex items-center gap-2">
        <Badge variant="outline" className="shrink-0 font-mono">
          {index + 1}
        </Badge>
        <div className="flex-1">
          <Select
            value={action.action}
            onValueChange={(type: IrActionType) => onChange(makeDefaultAction(type, sources))}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {ACTION_TYPES.map((type) => (
                <SelectItem key={type} value={type}>
                  {t(`remote.actionTypes.${type}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={() => onMove(-1)}
          disabled={index === 0}
          title={t("remote.moveUp")}
        >
          <ChevronUp className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={() => onMove(1)}
          disabled={index === count - 1}
          title={t("remote.moveDown")}
        >
          <ChevronDown className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="text-destructive hover:text-destructive"
          onClick={onRemove}
          title={t("remote.removeAction")}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>

      {action.action === "nabaztag" && (
        <div className="space-y-2">
          <Label>{t("remote.fields.command")}</Label>
          <Input
            value={action.command}
            onChange={(e) => onChange({ ...action, command: e.target.value })}
            placeholder={t("remote.fields.commandPlaceholder")}
          />
          <p className="text-xs text-muted-foreground">{t("remote.fields.commandHint")}</p>
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs text-muted-foreground">{t("remote.fields.presets")}</span>
            {NABAZTAG_PRESETS.map((preset) => (
              <Button
                key={preset}
                type="button"
                variant="outline"
                size="sm"
                className="h-7 font-mono text-xs"
                onClick={() => onChange({ ...action, command: preset })}
              >
                {preset}
              </Button>
            ))}
          </div>
        </div>
      )}

      {action.action === "zigbee_power" && (
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-2">
            <Label>{t("remote.fields.lamp")}</Label>
            <ZigbeeLampSelect
              value={action.lamp}
              lamps={sources.lamps}
              onChange={(lamp) => onChange({ ...action, lamp })}
            />
          </div>
          <div className="space-y-2">
            <Label>{t("remote.fields.state")}</Label>
            <SwitchStateSelect
              value={action.state}
              onChange={(state) => onChange({ ...action, state })}
            />
          </div>
        </div>
      )}

      {action.action === "zigbee_brightness" && (
        <div className="space-y-3">
          <div className="space-y-2">
            <Label>{t("remote.fields.lamp")}</Label>
            <ZigbeeLampSelect
              value={action.lamp}
              lamps={sources.lamps}
              onChange={(lamp) => onChange({ ...action, lamp })}
            />
          </div>
          <div className="space-y-2">
            <Label>{t("remote.fields.brightness")}</Label>
            <div className="flex items-center gap-4">
              <Slider
                value={[action.brightness]}
                onValueChange={(value) => onChange({ ...action, brightness: value[0] })}
                min={0}
                max={254}
                step={1}
                className="flex-1"
              />
              <Badge variant="secondary" className="min-w-12 justify-center">
                {action.brightness}
              </Badge>
            </div>
          </div>
        </div>
      )}

      {action.action === "broadlink_code" && (
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-2">
            <Label>{t("remote.fields.host")}</Label>
            <Select
              value={action.host || undefined}
              onValueChange={(host) => onChange({ ...action, host })}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("remote.fields.hostPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {sources.hosts.map((device) => (
                  <SelectItem key={device.host} value={device.host}>
                    {device.name} ({device.host})
                  </SelectItem>
                ))}
                {action.host && !sources.hosts.some((d) => d.host === action.host) && (
                  <SelectItem value={action.host}>{action.host}</SelectItem>
                )}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>{t("remote.fields.code")}</Label>
            <Select
              value={action.code_id || undefined}
              onValueChange={(code_id) => onChange({ ...action, code_id })}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("remote.fields.codePlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {sources.codes.map((code) => (
                  <SelectItem key={code.id} value={code.id}>
                    {code.name}
                  </SelectItem>
                ))}
                {action.code_id && !sources.codes.some((c) => c.id === action.code_id) && (
                  <SelectItem value={action.code_id}>{action.code_id}</SelectItem>
                )}
              </SelectContent>
            </Select>
          </div>
        </div>
      )}

      {action.action === "meross_power" && (
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-2">
            <Label>{t("remote.fields.device")}</Label>
            <Select
              value={action.device || undefined}
              onValueChange={(device) => onChange({ ...action, device })}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("remote.fields.devicePlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {sources.plugs.map((plug) => (
                  <SelectItem key={plug.id} value={plug.id}>
                    {plug.name} ({plug.ip})
                  </SelectItem>
                ))}
                {action.device && !sources.plugs.some((p) => p.id === action.device) && (
                  <SelectItem value={action.device}>{action.device}</SelectItem>
                )}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>{t("remote.fields.state")}</Label>
            <SwitchStateSelect
              value={action.state}
              onChange={(state) => onChange({ ...action, state })}
            />
          </div>
        </div>
      )}

      {action.action === "climate_toggle" && (
        <ClimateToggleFields action={action} sources={sources} onChange={onChange} />
      )}
    </div>
  );
}

function ClimateToggleFields({
  action,
  sources,
  onChange,
}: {
  action: Extract<IrAction, { action: "climate_toggle" }>;
  sources: IrActionSources;
  onChange: (action: IrAction) => void;
}) {
  const { t } = useTranslation();
  const settings = parseClimateCommand(action.on_command);

  const update = (patch: Partial<ClimateSettingsForm>) => {
    if (!settings) return;
    onChange({ ...action, on_command: buildClimateCommand({ ...settings, ...patch }) });
  };

  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">{t("remote.fields.climateHint")}</p>
      <div className="space-y-2">
        <Label>{t("remote.fields.host")}</Label>
        <Select
          value={action.host || undefined}
          onValueChange={(host) => onChange({ ...action, host })}
        >
          <SelectTrigger>
            <SelectValue placeholder={t("remote.fields.hostPlaceholder")} />
          </SelectTrigger>
          <SelectContent>
            {sources.hosts.map((device) => (
              <SelectItem key={device.host} value={device.host}>
                {device.name} ({device.host})
              </SelectItem>
            ))}
            {action.host && !sources.hosts.some((d) => d.host === action.host) && (
              <SelectItem value={action.host}>{action.host}</SelectItem>
            )}
          </SelectContent>
        </Select>
      </div>
      {settings ? (
        <>
          <div className="grid gap-3 sm:grid-cols-3">
            <div className="space-y-2">
              <Label>{t("remote.fields.climateMode")}</Label>
              <Select value={settings.mode} onValueChange={(mode) => update({ mode })}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CLIMATE_MODES.map((mode) => (
                    <SelectItem key={mode} value={mode}>
                      {t(`remote.fields.climateModes.${mode}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>{t("remote.fields.climateFan")}</Label>
              <Select value={settings.fan} onValueChange={(fan) => update({ fan })}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CLIMATE_FANS.map((fan) => (
                    <SelectItem key={fan} value={fan}>
                      {t(`remote.fields.climateFans.${fan}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>{t("remote.fields.climateVane")}</Label>
              <Select value={settings.vane} onValueChange={(vane) => update({ vane })}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CLIMATE_VANES.map((vane) => (
                    <SelectItem key={vane} value={vane}>
                      {t(`remote.fields.climateVanes.${vane}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="space-y-2">
            <Label>{t("remote.fields.climateTemperature")}</Label>
            <div className="flex items-center gap-4">
              <Slider
                value={[settings.temperature]}
                onValueChange={(value) => update({ temperature: value[0] })}
                min={16}
                max={31}
                step={1}
                className="flex-1"
              />
              <Badge variant="secondary" className="min-w-12 justify-center">
                {settings.temperature}°C
              </Badge>
            </div>
          </div>
        </>
      ) : (
        // Advanced command (econo/timer/…) the pickers can't represent:
        // keep it editable as raw text instead of destroying it.
        <div className="space-y-2">
          <Label>{t("remote.fields.climateRawCommand")}</Label>
          <Input
            value={action.on_command}
            onChange={(e) => onChange({ ...action, on_command: e.target.value })}
            className="font-mono"
          />
        </div>
      )}
    </div>
  );
}

interface IrBindingEditorProps {
  /** Fixed key code when editing an existing binding; undefined = capture flow. */
  initialCode?: number;
  keymap: Record<string, IrBinding>;
  onSaved: () => void;
  onCancel: () => void;
}

export function IrBindingEditor({ initialCode, keymap, onSaved, onCancel }: IrBindingEditorProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const sources = useIrActionSources();

  const initialBinding = initialCode !== undefined ? keymap[String(initialCode)] : undefined;

  const [code, setCode] = useState<number | null>(initialCode ?? null);
  const [codeText, setCodeText] = useState(initialCode !== undefined ? String(initialCode) : "");
  const [label, setLabel] = useState(initialBinding?.label ?? "");
  const [repeat, setRepeat] = useState(initialBinding?.repeat ?? false);
  const [actions, setActions] = useState<IrAction[]>(initialBinding?.actions ?? []);
  const [capturing, setCapturing] = useState(initialCode === undefined);
  const [testResults, setTestResults] = useState<string[] | null>(null);

  // Only prefill from an existing binding while the user has not edited the
  // form yet — a captured key must not clobber work in progress.
  const dirtyRef = useRef(false);
  // Events received before the capture started must not be picked up: the
  // baseline is the newest event seen on the first poll (server clock, so no
  // browser/server skew issues).
  const captureBaselineRef = useRef<string | null>(null);

  const recentQuery = useQuery({
    queryKey: ["ir-recent"],
    queryFn: irApi.recent,
    refetchInterval: 1000,
    enabled: capturing,
  });

  const recentEvents = recentQuery.data?.events;
  useEffect(() => {
    if (!capturing || !recentEvents) return;
    if (captureBaselineRef.current === null) {
      captureBaselineRef.current = recentEvents[0]?.receivedAt ?? "";
      return;
    }
    const press = recentEvents.find(
      (event) => event.value === 1 && event.receivedAt > (captureBaselineRef.current ?? ""),
    );
    if (press) {
      setCode(press.code);
      setCodeText(String(press.code));
      setCapturing(false);
    }
  }, [capturing, recentEvents]);

  // A captured/typed key that is already mapped edits the existing binding.
  useEffect(() => {
    if (code === null || code === initialCode || dirtyRef.current) return;
    const existing = keymap[String(code)];
    if (existing) {
      setLabel(existing.label ?? "");
      setRepeat(existing.repeat ?? false);
      setActions(existing.actions);
    }
  }, [code, initialCode, keymap]);

  const markDirty = () => {
    dirtyRef.current = true;
  };

  const startCapture = () => {
    captureBaselineRef.current = null;
    setCapturing(true);
  };

  const onManualCode = (text: string) => {
    setCodeText(text);
    setCapturing(false);
    const parsed = Number.parseInt(text, 10);
    setCode(Number.isInteger(parsed) && parsed >= 0 ? parsed : null);
  };

  const alreadyMapped = code !== null && code !== initialCode && keymap[String(code)] !== undefined;

  // Testing dry-runs the actions and needs no key code; saving needs both.
  const validate = (requireCode: boolean): boolean => {
    if (requireCode && code === null) {
      toast({
        title: t("common.error"),
        description: t("remote.missingCode"),
        variant: "destructive",
      });
      return false;
    }
    if (actions.length === 0) {
      toast({
        title: t("common.error"),
        description: t("remote.missingActions"),
        variant: "destructive",
      });
      return false;
    }
    if (!actions.every(isActionComplete)) {
      toast({
        title: t("common.error"),
        description: t("remote.incompleteActions"),
        variant: "destructive",
      });
      return false;
    }
    return true;
  };

  const testMutation = useMutation({
    mutationFn: () => irApi.test(actions),
    onSuccess: (response) => {
      setTestResults(response.results);
      toast({
        title: response.success ? t("remote.testResults") : t("remote.testFailed"),
        description: response.message,
        variant: response.success ? "default" : "destructive",
      });
    },
    onError: (error) => {
      setTestResults(null);
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("remote.testFailed"),
        variant: "destructive",
      });
    },
  });

  const saveMutation = useMutation({
    mutationFn: (binding: IrBinding) => irApi.setBinding(code as number, binding),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["ir-keymap"] });
      toast({
        title: t("remote.saved"),
        description: t("remote.keyBadge", { code }),
      });
      onSaved();
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("remote.saveFailed"),
        variant: "destructive",
      });
    },
  });

  const updateAction = (index: number, action: IrAction) => {
    markDirty();
    setActions((current) => current.map((a, i) => (i === index ? action : a)));
    setTestResults(null);
  };

  const moveAction = (index: number, direction: -1 | 1) => {
    markDirty();
    setActions((current) => {
      const next = [...current];
      const [moved] = next.splice(index, 1);
      next.splice(index + direction, 0, moved);
      return next;
    });
    setTestResults(null);
  };

  const removeAction = (index: number) => {
    markDirty();
    setActions((current) => current.filter((_, i) => i !== index));
    setTestResults(null);
  };

  const addAction = () => {
    markDirty();
    setActions((current) => [...current, makeDefaultAction("nabaztag", sources)]);
    setTestResults(null);
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!validate(true)) return;
    saveMutation.mutate({
      actions,
      label: label.trim() || undefined,
      repeat,
    });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-6">
      {/* Key code — capture from the remote or type it manually */}
      <div className="space-y-2">
        <Label className="flex items-center gap-2">
          <Radio className="h-4 w-4" />
          {t("remote.keyCode")}
        </Label>
        {initialCode === undefined ? (
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant={capturing ? "secondary" : "outline"}
                onClick={() => (capturing ? setCapturing(false) : startCapture())}
                className="flex-1"
              >
                {capturing ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    {t("remote.stopCapture")}
                  </>
                ) : (
                  <>
                    <Radio className="mr-2 h-4 w-4" />
                    {t("remote.capture")}
                  </>
                )}
              </Button>
              <Input
                inputMode="numeric"
                value={codeText}
                onChange={(e) => onManualCode(e.target.value)}
                placeholder={t("remote.manualCodePlaceholder")}
                className="w-40 font-mono"
              />
            </div>
            {capturing && (
              <p className="text-sm text-muted-foreground animate-pulse">{t("remote.capturing")}</p>
            )}
          </div>
        ) : (
          <Badge variant="outline" className="text-base">
            {remoteKeyLabel(initialCode) ?? t("remote.keyBadge", { code: initialCode })}
            <span className="ml-2 font-mono text-xs opacity-60">#{initialCode}</span>
          </Badge>
        )}
        {alreadyMapped && (
          <p className="text-sm text-amber-600 dark:text-amber-400">{t("remote.alreadyMapped")}</p>
        )}
      </div>

      {/* Label */}
      <div className="space-y-2">
        <Label>{t("remote.label")}</Label>
        <Input
          value={label}
          onChange={(e) => {
            markDirty();
            setLabel(e.target.value);
          }}
          placeholder={t("remote.labelPlaceholder")}
        />
      </div>

      {/* Repeat */}
      <div className="flex items-center justify-between rounded-lg border p-4">
        <div className="space-y-0.5">
          <Label>{t("remote.repeat")}</Label>
          <p className="text-sm text-muted-foreground">{t("remote.repeatHint")}</p>
        </div>
        <Switch
          checked={repeat}
          onCheckedChange={(checked) => {
            markDirty();
            setRepeat(checked);
          }}
        />
      </div>

      {/* Actions */}
      <div className="space-y-3">
        <div className="space-y-0.5">
          <Label>{t("remote.actions")}</Label>
          <p className="text-sm text-muted-foreground">{t("remote.actionsHint")}</p>
        </div>
        {actions.length === 0 && (
          <p className="text-sm text-muted-foreground">{t("remote.missingActions")}</p>
        )}
        {actions.map((action, index) => (
          <ActionRow
            key={index}
            action={action}
            index={index}
            count={actions.length}
            sources={sources}
            onChange={(updated) => updateAction(index, updated)}
            onMove={(direction) => moveAction(index, direction)}
            onRemove={() => removeAction(index)}
          />
        ))}
        <Button type="button" variant="outline" size="sm" onClick={addAction}>
          <Plus className="mr-2 h-4 w-4" />
          {t("remote.addAction")}
        </Button>
      </div>

      {/* Test results */}
      {testResults && (
        <div className="space-y-1 rounded-lg border p-3">
          <p className="text-sm font-medium">{t("remote.testResults")}</p>
          {testResults.map((result, index) => (
            <p
              key={index}
              className={`font-mono text-xs ${
                result.startsWith("failed") ? "text-destructive" : "text-muted-foreground"
              }`}
            >
              {result}
            </p>
          ))}
        </div>
      )}

      {/* Actions */}
      <div className="flex gap-3">
        <Button type="button" variant="outline" onClick={onCancel} className="flex-1">
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant="secondary"
          className="flex-1"
          onClick={() => {
            if (validate(false)) testMutation.mutate();
          }}
          disabled={testMutation.isPending}
        >
          {testMutation.isPending ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <FlaskConical className="mr-2 h-4 w-4" />
          )}
          {t("remote.test")}
        </Button>
        <Button type="submit" className="flex-1" disabled={saveMutation.isPending}>
          {saveMutation.isPending ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <Save className="mr-2 h-4 w-4" />
          )}
          {t("common.save")}
        </Button>
      </div>
    </form>
  );
}
