import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { tempoApi, TempoPrediction } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { TempoCalendar } from "@/components/devices/TempoCalendar";
import {
  ArrowLeft,
  Calendar,
  TrendingUp,
  AlertCircle,
  Zap,
  Snowflake,
  Sun,
  CloudSnow,
} from "lucide-react";

const colorConfig = {
  BLUE: {
    bg: "bg-blue-500",
    text: "text-blue-600",
    border: "border-blue-500",
    light: "bg-blue-50",
    icon: "💙",
  },
  WHITE: {
    bg: "bg-gray-200 dark:bg-gray-600",
    text: "text-gray-600 dark:text-gray-300",
    border: "border-gray-400 dark:border-gray-600",
    light: "bg-gray-50 dark:bg-gray-800",
    icon: "🤍",
  },
  RED: {
    bg: "bg-red-500",
    text: "text-red-600",
    border: "border-red-500",
    light: "bg-red-50",
    icon: "❤️",
  },
};

function PredictionCard({ prediction, index }: { prediction: TempoPrediction; index: number }) {
  const { t, i18n } = useTranslation();
  const config = colorConfig[prediction.predicted_color];

  const date = new Date(prediction.date);
  const isToday = index === 0;
  const isTomorrow = index === 1;

  const dayLabel = isToday
    ? t("tempo.today")
    : isTomorrow
      ? t("tempo.tomorrow")
      : date.toLocaleDateString(i18n.language, { weekday: "long" });

  const dateLabel = date.toLocaleDateString(i18n.language, {
    day: "numeric",
    month: "short",
  });

  const confidencePercent = Math.round(prediction.confidence * 100);

  return (
    <Card
      className={`relative overflow-hidden border-2 ${config.border} transition-all hover:shadow-lg`}
    >
      {/* Color indicator stripe */}
      <div className={`absolute top-0 left-0 right-0 h-2 ${config.bg}`} />

      <CardHeader className="pb-2 pt-4">
        <div className="flex justify-between items-start">
          <div>
            <p className="text-sm text-muted-foreground capitalize">{dayLabel}</p>
            <p className="text-lg font-semibold">{dateLabel}</p>
          </div>
          <Badge
            className={`${config.bg} ${prediction.predicted_color === "WHITE" ? "text-gray-700 dark:text-gray-100" : "text-white"} text-lg px-3 py-1`}
          >
            {config.icon} {t(`tempo.colors.${prediction.predicted_color.toLowerCase()}`)}
          </Badge>
        </div>
      </CardHeader>

      <CardContent className="space-y-3">
        {/* Confidence */}
        <div className="space-y-1">
          <div className="flex justify-between text-sm">
            <span className="text-muted-foreground">{t("tempo.prediction.confidence")}</span>
            <span className="font-medium">{confidencePercent}%</span>
          </div>
          <Progress value={confidencePercent} className="h-2" />
        </div>

        {/* Probabilities */}
        <div className="grid grid-cols-3 gap-2 text-center text-xs">
          <div className="p-2 rounded bg-blue-50 dark:bg-blue-950/40">
            <p className="text-blue-600 dark:text-blue-300 font-medium">
              {Math.round(prediction.probabilities.BLUE * 100)}%
            </p>
            <p className="text-muted-foreground">{t("tempo.colors.blue")}</p>
          </div>
          <div className="p-2 rounded bg-gray-100 dark:bg-gray-800">
            <p className="text-gray-600 dark:text-gray-300 font-medium">
              {Math.round(prediction.probabilities.WHITE * 100)}%
            </p>
            <p className="text-muted-foreground">{t("tempo.colors.white")}</p>
          </div>
          <div className="p-2 rounded bg-red-50 dark:bg-red-950/40">
            <p className="text-red-600 dark:text-red-300 font-medium">
              {Math.round(prediction.probabilities.RED * 100)}%
            </p>
            <p className="text-muted-foreground">{t("tempo.colors.red")}</p>
          </div>
        </div>

        {/* Constraints indicators */}
        <div className="flex gap-2 flex-wrap">
          {prediction.constraints.is_in_red_period ? (
            <Badge variant="outline" className="text-xs gap-1">
              <Snowflake className="h-3 w-3" />
              {t("tempo.prediction.redPeriod")}
            </Badge>
          ) : (
            <Badge variant="outline" className="text-xs gap-1 opacity-50">
              <Sun className="h-3 w-3" />
              {t("tempo.prediction.outsideRedPeriod")}
            </Badge>
          )}
          {!prediction.constraints.can_be_red && (
            <Badge variant="secondary" className="text-xs gap-1">
              <AlertCircle className="h-3 w-3" />
              {t("tempo.prediction.redBlocked")}
            </Badge>
          )}
          {!prediction.constraints.can_be_white && (
            <Badge variant="secondary" className="text-xs gap-1">
              <AlertCircle className="h-3 w-3" />
              {t("tempo.prediction.whiteBlocked")}
            </Badge>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function StateCard({
  state,
}: {
  state: {
    season: string;
    stock_red_remaining: number;
    stock_red_total: number;
    stock_white_remaining: number;
    stock_white_total: number;
  };
}) {
  const { t } = useTranslation();

  const redUsed = state.stock_red_total - state.stock_red_remaining;
  const whiteUsed = state.stock_white_total - state.stock_white_remaining;
  const redPercent = (state.stock_red_remaining / state.stock_red_total) * 100;
  const whitePercent = (state.stock_white_remaining / state.stock_white_total) * 100;

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-lg flex items-center gap-2">
          <Calendar className="h-5 w-5" />
          {t("tempo.prediction.season")} {state.season}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Red days stock */}
        <div className="space-y-2">
          <div className="flex justify-between text-sm">
            <span className="flex items-center gap-2">
              <span className="w-3 h-3 rounded-full bg-red-500" />
              {t("tempo.prediction.redDays")}
            </span>
            <span className="font-medium">
              {redUsed} / {state.stock_red_total} {t("tempo.prediction.used")}
            </span>
          </div>
          <Progress
            value={100 - redPercent}
            className="h-3 bg-red-100 dark:bg-red-950/50 [&>div]:bg-red-500"
          />
          <p className="text-xs text-muted-foreground">
            {state.stock_red_remaining} {t("tempo.prediction.remaining")}
          </p>
        </div>

        {/* White days stock */}
        <div className="space-y-2">
          <div className="flex justify-between text-sm">
            <span className="flex items-center gap-2">
              <span className="w-3 h-3 rounded-full bg-gray-400 dark:bg-gray-500" />
              {t("tempo.prediction.whiteDays")}
            </span>
            <span className="font-medium">
              {whiteUsed} / {state.stock_white_total} {t("tempo.prediction.used")}
            </span>
          </div>
          <Progress
            value={100 - whitePercent}
            className="h-3 bg-gray-200 dark:bg-gray-700 [&>div]:bg-gray-500"
          />
          <p className="text-xs text-muted-foreground">
            {state.stock_white_remaining} {t("tempo.prediction.remaining")}
          </p>
        </div>
      </CardContent>
    </Card>
  );
}

function LoadingSkeleton() {
  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
        {[1, 2, 3, 4, 5, 6, 7].map((i) => (
          <Card key={i}>
            <CardHeader>
              <Skeleton className="h-4 w-20" />
              <Skeleton className="h-6 w-16" />
            </CardHeader>
            <CardContent className="space-y-3">
              <Skeleton className="h-2 w-full" />
              <div className="grid grid-cols-3 gap-2">
                <Skeleton className="h-12 w-full" />
                <Skeleton className="h-12 w-full" />
                <Skeleton className="h-12 w-full" />
              </div>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}

export default function TempoPredictionPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();

  const { data: predictionsData, isLoading: loadingPredictions } = useQuery({
    queryKey: ["tempo-predictions"],
    queryFn: tempoApi.getPredictions,
    refetchInterval: 1000 * 60 * 30, // Refresh every 30 minutes
    staleTime: 1000 * 60 * 15, // Consider stale after 15 minutes
  });

  const { data: stateData, isLoading: loadingState } = useQuery({
    queryKey: ["tempo-state"],
    queryFn: tempoApi.getState,
    refetchInterval: 1000 * 60 * 60, // Refresh every hour
    staleTime: 1000 * 60 * 30, // Consider stale after 30 minutes
  });

  const isLoading = loadingPredictions || loadingState;
  const hasError =
    (predictionsData && !predictionsData.success) || (stateData && !stateData.success);

  return (
    <div className="container max-w-6xl mx-auto py-6 px-4 space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <Button variant="ghost" size="icon" onClick={() => navigate("/")} className="shrink-0">
          <ArrowLeft className="h-5 w-5" />
        </Button>
        <div className="flex-1">
          <h1 className="text-2xl font-bold flex items-center gap-2">
            <TrendingUp className="h-6 w-6" />
            {t("tempo.prediction.title")}
          </h1>
          <p className="text-muted-foreground">{t("tempo.prediction.subtitle")}</p>
        </div>
        <Badge variant="outline" className="gap-1">
          <Zap className="h-3 w-3" />
          {predictionsData?.model_version || "v1.0"}
        </Badge>
      </div>

      {/* Error state */}
      {hasError && (
        <Card className="border-destructive">
          <CardContent className="py-4">
            <div className="flex items-center gap-2 text-destructive">
              <AlertCircle className="h-5 w-5" />
              <span>
                {predictionsData?.error || stateData?.error || t("tempo.prediction.error")}
              </span>
            </div>
            <p className="text-sm text-muted-foreground mt-1">{t("tempo.prediction.errorHint")}</p>
          </CardContent>
        </Card>
      )}

      {/* Loading state */}
      {isLoading && <LoadingSkeleton />}

      {/* Content */}
      {!isLoading && predictionsData?.success && (
        <>
          {/* Tempo Calendar - Full Season View */}
          <TempoCalendar />

          {/* State card */}
          {predictionsData.state && <StateCard state={predictionsData.state} />}

          {/* Predictions grid */}
          <div>
            <h2 className="text-lg font-semibold mb-4 flex items-center gap-2">
              <CloudSnow className="h-5 w-5" />
              {t("tempo.prediction.weekForecast")}
            </h2>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
              {predictionsData.predictions?.map((prediction, index) => (
                <PredictionCard key={prediction.date} prediction={prediction} index={index} />
              ))}
            </div>
          </div>

          {/* Explanation */}
          <Card className="bg-muted/50">
            <CardContent className="py-4 text-sm text-muted-foreground">
              <p className="font-medium mb-2">{t("tempo.prediction.howItWorks")}</p>
              <p>{t("tempo.prediction.explanation")}</p>
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
