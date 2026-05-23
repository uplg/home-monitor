import type { ReactNode } from "react";

interface DashboardSectionHeaderProps {
  icon: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  iconClassName?: string;
}

export function DashboardSectionHeader({
  icon,
  title,
  description,
  actions,
  iconClassName = "bg-muted text-muted-foreground",
}: DashboardSectionHeaderProps) {
  return (
    <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
      <div className="flex items-center gap-3">
        <div
          className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl ${iconClassName}`}
        >
          {icon}
        </div>
        <div className="min-w-0 space-y-0.5">
          <div className="text-[1.1rem] font-semibold tracking-[-0.02em] text-foreground">
            {title}
          </div>
          {description ? <p className="text-sm text-muted-foreground">{description}</p> : null}
        </div>
      </div>
      {actions ? <div className="flex flex-wrap gap-2">{actions}</div> : null}
    </div>
  );
}
