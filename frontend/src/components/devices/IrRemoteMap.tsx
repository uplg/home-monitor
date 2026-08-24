import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import type { IrBinding } from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Circle,
  Delete,
  FastForward,
  Home,
  Info,
  Menu,
  Minus,
  Pause,
  Play,
  Plus,
  Power,
  Rewind,
  Search,
  Square,
  Text,
} from "lucide-react";

interface RemoteKey {
  /** Linux keycode delivered by the STB's KIR driver (kernel/bcm7231-kir.c). */
  code: number;
  label: string;
  icon?: ComponentType<{ className?: string }>;
}

/** Physical layout of the AirTies Ruwido remote, row by row. */
const REMOTE_ROWS: RemoteKey[][] = [
  [{ code: 116, label: "Power", icon: Power }],
  [
    { code: 2, label: "1" },
    { code: 3, label: "2" },
    { code: 4, label: "3" },
  ],
  [
    { code: 5, label: "4" },
    { code: 6, label: "5" },
    { code: 7, label: "6" },
  ],
  [
    { code: 8, label: "7" },
    { code: 9, label: "8" },
    { code: 10, label: "9" },
  ],
  [
    { code: 59, label: "F1" },
    { code: 11, label: "0" },
    { code: 60, label: "F2" },
  ],
  [
    { code: 14, label: "Erase", icon: Delete },
    { code: 103, label: "▲", icon: ChevronUp },
    { code: 102, label: "Home", icon: Home },
  ],
  [
    { code: 105, label: "◀", icon: ChevronLeft },
    { code: 353, label: "OK" },
    { code: 106, label: "▶", icon: ChevronRight },
  ],
  [
    { code: 388, label: "Txt", icon: Text },
    { code: 108, label: "▼", icon: ChevronDown },
    { code: 358, label: "i", icon: Info },
  ],
  [
    { code: 139, label: "Menu", icon: Menu },
    { code: 365, label: "EPG" },
    { code: 226, label: "ZAP" },
    { code: 217, label: "Search", icon: Search },
  ],
  [
    { code: 115, label: "Vol+", icon: Plus },
    { code: 167, label: "Rec", icon: Circle },
    { code: 402, label: "P+" },
  ],
  [
    { code: 114, label: "Vol−", icon: Minus },
    { code: 128, label: "Stop", icon: Square },
    { code: 403, label: "P−" },
  ],
  [
    { code: 168, label: "Rew", icon: Rewind },
    { code: 207, label: "Play", icon: Play },
    { code: 119, label: "Pause", icon: Pause },
    { code: 159, label: "FF", icon: FastForward },
  ],
];

const KEY_BY_CODE = new Map(REMOTE_ROWS.flat().map((key) => [key.code, key]));

/** Physical button name for a keycode ("4", "OK", "Power"), if known. */
export function remoteKeyLabel(code: number): string | undefined {
  return KEY_BY_CODE.get(code)?.label;
}

interface IrRemoteMapProps {
  keymap: Record<string, IrBinding>;
  onSelect: (code: number) => void;
}

/** Clickable picture of the remote: mapped keys are highlighted, any key
 * opens its binding editor. */
export function IrRemoteMap({ keymap, onSelect }: IrRemoteMapProps) {
  const { t } = useTranslation();

  return (
    <div className="mx-auto w-fit space-y-2 rounded-3xl border bg-muted/30 p-4">
      {REMOTE_ROWS.map((row, rowIndex) => (
        <div
          key={rowIndex}
          className={cn("flex gap-2", rowIndex === 0 ? "justify-end" : "justify-center")}
        >
          {row.map((key) => {
            const binding = keymap[String(key.code)];
            const Icon = key.icon;
            const title = binding
              ? (binding.label ?? t("remote.keyBadge", { code: key.code }))
              : t("remote.mapKeyTitle", { label: key.label, code: key.code });

            return (
              <button
                key={key.code}
                type="button"
                title={title}
                onClick={() => onSelect(key.code)}
                className={cn(
                  "flex h-9 min-w-9 items-center justify-center gap-1 rounded-full border px-2",
                  "text-xs font-medium transition-colors",
                  binding
                    ? "border-primary bg-primary text-primary-foreground shadow-sm"
                    : "bg-background text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                )}
              >
                {Icon ? <Icon className="h-3.5 w-3.5" /> : key.label}
                {Icon && key.label.length > 2 && <span className="sr-only">{key.label}</span>}
              </button>
            );
          })}
        </div>
      ))}
      <p className="pt-1 text-center text-xs text-muted-foreground">{t("remote.visualHint")}</p>
    </div>
  );
}
