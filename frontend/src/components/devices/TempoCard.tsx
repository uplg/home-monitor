import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { tempoApi } from "@/lib/api";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { DashboardSectionHeader } from "@/components/dashboard/DashboardSectionHeader";
import { Zap, Calendar, ArrowRight, Euro, TrendingUp } from "lucide-react";

type TempoColor = "BLUE" | "WHITE" | "RED" | null;

const colorStyles: Record<string, { bg: string; text: string; border: string; light: string }> = {
  BLUE: {
    bg: "bg-blue-500",
    text: "text-white",
    border: "border-blue-600",
    light: "bg-blue-50 text-blue-700",
  },
  WHITE: {
    bg: "bg-gray-100",
    text: "text-gray-800",
    border: "border-gray-300",
    light: "bg-gray-50 text-gray-700",
  },
  RED: {
    bg: "bg-red-500",
    text: "text-white",
    border: "border-red-600",
    light: "bg-red-50 text-red-700",
  },
};

function ColorBadge({
  color,
  label,
  isPrediction,
}: {
  color: TempoColor;
  label: string;
  isPrediction?: boolean;
}) {
  const { t } = useTranslation();

  if (!color) {
    return (
      <div className="flex flex-col items-center gap-1">
        <span className="text-xs text-muted-foreground">{label}</span>
        <div className="flex h-12 w-12 items-center justify-center rounded-full border-2 border-dashed border-muted-foreground/30 bg-muted/50">
          <span className="text-xs text-muted-foreground">?</span>
        </div>
        <span className="text-xs font-medium text-muted-foreground">{t("tempo.unknown")}</span>
      </div>
    );
  }

  const styles = colorStyles[color];

  return (
    <div className="flex flex-col items-center gap-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <div
        className={`flex h-12 w-12 items-center justify-center rounded-full border-2 ${styles.bg} ${styles.text} ${styles.border} shadow-sm ${isPrediction ? "border-dashed opacity-80" : ""}`}
      >
        {isPrediction ? <TrendingUp className="h-5 w-5" /> : <Zap className="h-5 w-5" />}
      </div>
      <span className={`text-xs font-medium ${color === "WHITE" ? "text-gray-700" : ""}`}>
        {t(`tempo.colors.${color.toLowerCase()}`)}
        {isPrediction && <span className="text-muted-foreground"> *</span>}
      </span>
    </div>
  );
}

function TempoSkeleton() {
  return (
    <Card className="border-0 bg-transparent shadow-none">
      <CardHeader className="px-0 pb-2">
        <div className="flex items-center gap-2">
          <Skeleton className="h-8 w-8 rounded-lg" />
          <Skeleton className="h-5 w-24" />
        </div>
      </CardHeader>
      <CardContent className="px-0">
        <div className="rounded-2xl bg-card/80 px-4 py-4 shadow-sm">
          <div className="flex items-center justify-center gap-4">
            <div className="flex flex-col items-center gap-1">
              <Skeleton className="h-3 w-12" />
              <Skeleton className="h-12 w-12 rounded-full" />
              <Skeleton className="h-3 w-10" />
            </div>
            <Skeleton className="h-4 w-4" />
            <div className="flex flex-col items-center gap-1">
              <Skeleton className="h-3 w-12" />
              <Skeleton className="h-12 w-12 rounded-full" />
              <Skeleton className="h-3 w-10" />
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

export function TempoCard() {
  const { t } = useTranslation();
  const navigate = useNavigate();

  const { data, isLoading, error } = useQuery({
    queryKey: ["tempo"],
    queryFn: tempoApi.get,
    refetchInterval: 30 * 60 * 1000, // Refresh every 30 minutes
    staleTime: 10 * 60 * 1000, // Consider data stale after 10 minutes
    retry: 2,
  });

  // Fetch predictions as fallback when official colors are not available
  const { data: predictionsData } = useQuery({
    queryKey: ["tempo-predictions"],
    queryFn: tempoApi.getPredictions,
    refetchInterval: 30 * 60 * 1000,
    staleTime: 15 * 60 * 1000,
    // Only fetch if we might need predictions
    enabled: !!data?.success,
  });

  if (isLoading) {
    return <TempoSkeleton />;
  }

  // Don't render the card if RTE credentials are not configured
  if (error || !data?.success) {
    // Silently return null - Tempo is optional
    return null;
  }

  const formatDate = (dateStr: string | undefined) => {
    if (!dateStr) return "";
    const date = new Date(dateStr);
    return date.toLocaleDateString("fr-FR", { weekday: "short", day: "numeric", month: "short" });
  };

  const formatPrice = (price: number) => {
    return (price * 100).toFixed(2); // Convert €/kWh to c€/kWh
  };

  // Get prediction for a specific date
  const getPredictionForDate = (dateStr: string | undefined) => {
    if (!dateStr || !predictionsData?.predictions) return null;
    const targetDate = dateStr.split("T")[0]; // Normalize to YYYY-MM-DD
    return predictionsData.predictions.find((p) => p.date === targetDate);
  };

  // Determine colors to display (official or prediction fallback)
  const todayColor = data.today?.color as TempoColor;
  const todayPrediction = !todayColor ? getPredictionForDate(data.today?.date) : null;
  const displayTodayColor = todayColor || (todayPrediction?.predicted_color as TempoColor);
  const isTodayPrediction = !todayColor && !!todayPrediction;

  const tomorrowColor = data.tomorrow?.color as TempoColor;
  const tomorrowPrediction = !tomorrowColor ? getPredictionForDate(data.tomorrow?.date) : null;
  const displayTomorrowColor = tomorrowColor || (tomorrowPrediction?.predicted_color as TempoColor);
  const isTomorrowPrediction = !tomorrowColor && !!tomorrowPrediction;

  // Get current tariff based on today's color
  const getCurrentTarif = () => {
    if (!data.tarifs || !displayTodayColor) return null;
    const colorKey = displayTodayColor.toLowerCase() as "blue" | "white" | "red";
    return data.tarifs[colorKey];
  };

  const currentTarif = getCurrentTarif();
  const dashboardPredictions =
    predictionsData?.predictions
      ?.filter((prediction) => {
        const predictionDate = new Date(`${prediction.date}T00:00:00`);
        const tomorrow = new Date();
        tomorrow.setHours(0, 0, 0, 0);
        tomorrow.setDate(tomorrow.getDate() + 2);
        return predictionDate >= tomorrow;
      })
      .slice(0, 4) ?? [];

  return (
    <Card className="border-0 bg-transparent shadow-none">
      <CardHeader className="px-0 pb-1">
        <DashboardSectionHeader
          icon={<Calendar className="h-5 w-5" />}
          iconClassName="bg-muted text-muted-foreground"
          title={
            <button
              type="button"
              className="group flex items-center gap-2 text-left transition-colors hover:text-foreground"
              onClick={() => navigate("/tempo-predictions")}
            >
              <span>{t("tempo.title")}</span>
              <TrendingUp className="h-4 w-4 text-muted-foreground transition-colors group-hover:text-foreground" />
            </button>
          }
          description={t("tempo.subtitle")}
        />
      </CardHeader>
      <CardContent className="space-y-4 px-0 pb-0">
        {/* Colors section */}
        <div className="rounded-2xl bg-card/80 px-4 py-4 shadow-sm">
          <div className="flex items-center justify-center gap-6">
            <ColorBadge
              color={displayTodayColor}
              label={`${t("tempo.today")} ${formatDate(data.today?.date)}`}
              isPrediction={isTodayPrediction}
            />
            <ArrowRight className="h-4 w-4 text-muted-foreground" />
            <ColorBadge
              color={displayTomorrowColor}
              label={`${t("tempo.tomorrow")} ${formatDate(data.tomorrow?.date)}`}
              isPrediction={isTomorrowPrediction}
            />
          </div>

          {(isTodayPrediction || isTomorrowPrediction) && (
            <p className="mt-3 text-center text-xs text-muted-foreground">
              * {t("tempo.predictionNotice")}
            </p>
          )}
        </div>

        {dashboardPredictions.length > 0 && (
          <div className="rounded-2xl bg-muted p-4">
            <div className="mb-2 flex items-center gap-2 text-sm font-medium text-muted-foreground">
              <TrendingUp className="h-4 w-4" />
              {t("tempo.prediction.dashboardTitle")}
            </div>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
              {dashboardPredictions.map((prediction) => {
                const color = prediction.predicted_color as TempoColor;
                const styles = color ? colorStyles[color] : null;
                return (
                  <div
                    key={prediction.date}
                    className={`rounded-xl p-2 text-center ${styles?.light ?? "bg-card text-muted-foreground"}`}
                  >
                    <div className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
                      {formatDate(prediction.date)}
                    </div>
                    <div className="mt-1 text-sm font-semibold">
                      {color ? t(`tempo.colors.${color.toLowerCase()}`) : t("tempo.unknown")}
                    </div>
                    <div className="mt-1 text-[11px] text-muted-foreground">
                      {t("tempo.prediction.confidenceShort", {
                        value: Math.round(prediction.confidence * 100),
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Current tariff section */}
        {currentTarif && displayTodayColor && (
          <div
            className={`rounded-2xl p-4 ${colorStyles[displayTodayColor]?.light || "bg-gray-50"}`}
          >
            <div className="flex items-center justify-center gap-2 mb-2">
              <Euro className="h-4 w-4" />
              <span className="text-sm font-medium">
                {t("tempo.currentTarif")}
                {isTodayPrediction && (
                  <span className="text-muted-foreground"> ({t("tempo.estimated")})</span>
                )}
              </span>
            </div>
            <div className="flex justify-center gap-6 text-sm">
              <div className="text-center">
                <div className="text-xs text-muted-foreground">{t("tempo.offPeak")}</div>
                <div className="font-semibold">{formatPrice(currentTarif.hc)} c€/kWh</div>
              </div>
              <div className="text-center">
                <div className="text-xs text-muted-foreground">{t("tempo.peak")}</div>
                <div className="font-semibold">{formatPrice(currentTarif.hp)} c€/kWh</div>
              </div>
            </div>
          </div>
        )}

        {/* All tariffs summary */}
        {data.tarifs && (
          <div className="rounded-2xl bg-muted px-4 py-3 text-xs text-muted-foreground">
            <details className="cursor-pointer">
              <summary className="flex items-center gap-1 hover:text-foreground">
                {t("tempo.allTarifs")}
              </summary>
              <div className="mt-2 grid grid-cols-3 gap-2 text-center">
                <div className="rounded bg-blue-50 p-2">
                  <div className="font-medium text-blue-700">{t("tempo.colors.blue")}</div>
                  <div className="text-blue-600">HC: {formatPrice(data.tarifs.blue.hc)}</div>
                  <div className="text-blue-600">HP: {formatPrice(data.tarifs.blue.hp)}</div>
                </div>
                <div className="rounded bg-gray-100 p-2">
                  <div className="font-medium text-gray-700">{t("tempo.colors.white")}</div>
                  <div className="text-gray-600">HC: {formatPrice(data.tarifs.white.hc)}</div>
                  <div className="text-gray-600">HP: {formatPrice(data.tarifs.white.hp)}</div>
                </div>
                <div className="rounded bg-red-50 p-2">
                  <div className="font-medium text-red-700">{t("tempo.colors.red")}</div>
                  <div className="text-red-600">HC: {formatPrice(data.tarifs.red.hc)}</div>
                  <div className="text-red-600">HP: {formatPrice(data.tarifs.red.hp)}</div>
                </div>
              </div>
            </details>
          </div>
        )}

        {data.cached && (
          <p className="mt-2 text-center text-xs text-muted-foreground">{t("tempo.cached")}</p>
        )}
      </CardContent>
    </Card>
  );
}
