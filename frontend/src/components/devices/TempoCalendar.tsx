import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { tempoApi, TempoCalendarDay } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ChevronLeft,
  ChevronRight,
  Calendar as CalendarIcon,
  TrendingUp,
  Info,
} from "lucide-react";
import { useState, useMemo } from "react";

// Color configuration
const colorConfig = {
  BLUE: {
    bg: "bg-blue-500",
    bgLight: "bg-blue-100 dark:bg-blue-950/50",
    text: "text-blue-700 dark:text-blue-300",
    border: "border-blue-300 dark:border-blue-700",
    hover: "hover:bg-blue-200 dark:hover:bg-blue-900",
    ring: "ring-blue-400",
  },
  WHITE: {
    bg: "bg-gray-300 dark:bg-gray-500",
    bgLight: "bg-gray-100 dark:bg-gray-800",
    text: "text-gray-700 dark:text-gray-300",
    border: "border-gray-300 dark:border-gray-600",
    hover: "hover:bg-gray-200 dark:hover:bg-gray-700",
    ring: "ring-gray-400",
  },
  RED: {
    bg: "bg-red-500",
    bgLight: "bg-red-100 dark:bg-red-950/50",
    text: "text-red-700 dark:text-red-300",
    border: "border-red-300 dark:border-red-700",
    hover: "hover:bg-red-200 dark:hover:bg-red-900",
    ring: "ring-red-400",
  },
};

// French day names
const dayNames = ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"];
const dayNamesEn = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

// French month names
const monthNames = [
  "Janvier",
  "Février",
  "Mars",
  "Avril",
  "Mai",
  "Juin",
  "Juillet",
  "Août",
  "Septembre",
  "Octobre",
  "Novembre",
  "Décembre",
];
const monthNamesEn = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

interface CalendarDayProps {
  day: TempoCalendarDay;
  isToday: boolean;
}

function CalendarDay({ day, isToday }: CalendarDayProps) {
  const { t, i18n } = useTranslation();
  const date = new Date(day.date);
  const dayNum = date.getDate();

  if (!day.color) {
    return (
      <div className="aspect-square p-1">
        <div className="w-full h-full rounded-md bg-muted/30 flex items-center justify-center text-muted-foreground text-xs">
          {dayNum}
        </div>
      </div>
    );
  }

  const config = colorConfig[day.color];

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <div className="aspect-square p-0.5">
            <div
              className={`
                w-full h-full rounded-md flex flex-col items-center justify-center
                ${day.is_prediction ? config.bgLight : config.bg}
                ${day.is_prediction ? config.text : "text-white"}
                ${day.is_prediction ? "border border-dashed" : ""}
                ${day.is_prediction ? config.border : ""}
                ${isToday ? "ring-2 ring-offset-1" : ""}
                ${isToday ? config.ring : ""}
                transition-all cursor-pointer hover:scale-105
              `}
            >
              <span className={`text-xs font-medium ${isToday ? "font-bold" : ""}`}>{dayNum}</span>
              {day.is_prediction && day.confidence && (
                <span className="text-[8px] opacity-70">{Math.round(day.confidence * 100)}%</span>
              )}
            </div>
          </div>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-xs">
          <div className="space-y-1">
            <div className="font-medium">
              {date.toLocaleDateString(i18n.language, {
                weekday: "long",
                day: "numeric",
                month: "long",
              })}
            </div>
            <div className="flex items-center gap-2">
              <Badge
                className={`${config.bg} ${day.color === "WHITE" ? "text-gray-800 dark:text-gray-100" : "text-white"}`}
              >
                {t(`tempo.colors.${day.color.toLowerCase()}`)}
              </Badge>
              {day.is_prediction ? (
                <span className="text-xs text-muted-foreground flex items-center gap-1">
                  <TrendingUp className="h-3 w-3" />
                  {t("tempo.calendar.prediction")}
                </span>
              ) : (
                <span className="text-xs text-muted-foreground">{t("tempo.calendar.actual")}</span>
              )}
            </div>
            {day.probabilities && (
              <div className="grid grid-cols-3 gap-1 text-xs pt-1">
                <div className="text-blue-600 dark:text-blue-300">
                  B: {Math.round(day.probabilities.BLUE * 100)}%
                </div>
                <div className="text-gray-600 dark:text-gray-300">
                  W: {Math.round(day.probabilities.WHITE * 100)}%
                </div>
                <div className="text-red-600 dark:text-red-300">
                  R: {Math.round(day.probabilities.RED * 100)}%
                </div>
              </div>
            )}
            {day.constraints && !day.constraints.is_in_red_period && (
              <div className="text-xs text-muted-foreground">
                {t("tempo.prediction.outsideRedPeriod")}
              </div>
            )}
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

interface MonthCalendarProps {
  year: number;
  month: number; // 0-11
  calendarData: TempoCalendarDay[];
}

function MonthCalendar({ year, month, calendarData }: MonthCalendarProps) {
  const { i18n } = useTranslation();
  const isFr = i18n.language === "fr";

  // Get today's date for highlighting
  const today = new Date();
  const todayStr = today.toISOString().split("T")[0];

  // Build a map of date -> day data
  const dayMap = useMemo(() => {
    const map = new Map<string, TempoCalendarDay>();
    calendarData.forEach((day) => {
      map.set(day.date, day);
    });
    return map;
  }, [calendarData]);

  // Calculate first day of month and number of days
  const firstDay = new Date(year, month, 1);
  const lastDay = new Date(year, month + 1, 0);
  const daysInMonth = lastDay.getDate();

  // Get the day of week for the first day (0 = Sunday, convert to Monday = 0)
  let startDayOfWeek = firstDay.getDay() - 1;
  if (startDayOfWeek < 0) startDayOfWeek = 6;

  // Build calendar grid
  const weeks: (TempoCalendarDay | null)[][] = [];
  let currentWeek: (TempoCalendarDay | null)[] = [];

  // Add empty cells for days before the first day
  for (let i = 0; i < startDayOfWeek; i++) {
    currentWeek.push(null);
  }

  // Add days of the month
  for (let day = 1; day <= daysInMonth; day++) {
    const dateStr = `${year}-${String(month + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    const dayData = dayMap.get(dateStr);

    if (dayData) {
      currentWeek.push(dayData);
    } else {
      // Create placeholder for days not in tempo data
      currentWeek.push({
        date: dateStr,
        color: null,
        is_actual: false,
        is_prediction: false,
      });
    }

    if (currentWeek.length === 7) {
      weeks.push(currentWeek);
      currentWeek = [];
    }
  }

  // Add remaining days to last week
  if (currentWeek.length > 0) {
    while (currentWeek.length < 7) {
      currentWeek.push(null);
    }
    weeks.push(currentWeek);
  }

  const names = isFr ? dayNames : dayNamesEn;
  const monthName = isFr ? monthNames[month] : monthNamesEn[month];

  return (
    <div className="bg-card rounded-lg border p-3">
      <h3 className="text-sm font-medium text-center mb-2">
        {monthName} {year}
      </h3>
      <div className="grid grid-cols-7 gap-0.5">
        {/* Day headers */}
        {names.map((name) => (
          <div
            key={name}
            className="text-[10px] font-medium text-muted-foreground text-center py-1"
          >
            {name}
          </div>
        ))}
        {/* Calendar days */}
        {weeks.map((week, weekIndex) =>
          week.map((day, dayIndex) => {
            if (!day) {
              return <div key={`${weekIndex}-${dayIndex}`} className="aspect-square" />;
            }
            return <CalendarDay key={day.date} day={day} isToday={day.date === todayStr} />;
          }),
        )}
      </div>
    </div>
  );
}

// Get available seasons (from 2020 to current + 1)
function getAvailableSeasons(): string[] {
  const currentYear = new Date().getFullYear();
  const currentMonth = new Date().getMonth();
  const endYear = currentMonth >= 8 ? currentYear + 1 : currentYear;

  const seasons: string[] = [];
  for (let year = 2020; year < endYear; year++) {
    seasons.push(`${year}-${year + 1}`);
  }
  return seasons.reverse(); // Most recent first
}

export function TempoCalendar() {
  const { t } = useTranslation();

  // Get current season as default
  const currentDate = new Date();
  const currentYear = currentDate.getFullYear();
  const currentMonth = currentDate.getMonth();
  const defaultSeason =
    currentMonth >= 8 ? `${currentYear}-${currentYear + 1}` : `${currentYear - 1}-${currentYear}`;

  const [selectedSeason, setSelectedSeason] = useState(defaultSeason);
  const [viewMonth, setViewMonth] = useState(currentMonth);
  const [viewYear, setViewYear] = useState(currentYear);

  // Fetch calendar data
  const { data: calendarData, isLoading } = useQuery({
    queryKey: ["tempo-calendar", selectedSeason],
    queryFn: () => tempoApi.getCalendar(selectedSeason),
    refetchInterval: 1000 * 60 * 30, // Refresh every 30 minutes
    staleTime: 1000 * 60 * 15,
  });

  // Get months to display for the season
  const seasonMonths = useMemo(() => {
    const [startYear, endYear] = selectedSeason.split("-").map(Number);
    const months: { year: number; month: number }[] = [];

    // September to December of start year
    for (let m = 8; m <= 11; m++) {
      months.push({ year: startYear, month: m });
    }
    // January to August of end year
    for (let m = 0; m <= 7; m++) {
      months.push({ year: endYear, month: m });
    }

    return months;
  }, [selectedSeason]);

  // Navigate months
  const goToPrevMonth = () => {
    if (viewMonth === 0) {
      setViewMonth(11);
      setViewYear(viewYear - 1);
    } else {
      setViewMonth(viewMonth - 1);
    }
  };

  const goToNextMonth = () => {
    if (viewMonth === 11) {
      setViewMonth(0);
      setViewYear(viewYear + 1);
    } else {
      setViewMonth(viewMonth + 1);
    }
  };

  const goToToday = () => {
    setViewMonth(currentMonth);
    setViewYear(currentYear);
  };

  // Get visible months - only show months that have data (history + 7 days predictions)
  const visibleMonths = useMemo(() => {
    // For current season, only show up to current month + next month (predictions are only 7 days)
    const isCurrentSeason = selectedSeason === defaultSeason;

    if (isCurrentSeason) {
      // Show current month and next month only (predictions are max 7 days ahead)
      const months: { year: number; month: number }[] = [];
      months.push({ year: currentYear, month: currentMonth });

      // Add next month if we're within 7 days of month end
      const daysInCurrentMonth = new Date(currentYear, currentMonth + 1, 0).getDate();
      const currentDay = currentDate.getDate();
      if (currentDay + 7 > daysInCurrentMonth) {
        const nextMonth = currentMonth === 11 ? 0 : currentMonth + 1;
        const nextYear = currentMonth === 11 ? currentYear + 1 : currentYear;
        months.push({ year: nextYear, month: nextMonth });
      }

      return months;
    }

    // For past seasons, show all months in a 4-month grid
    const currentIndex = seasonMonths.findIndex(
      (m) => m.year === viewYear && m.month === viewMonth,
    );

    if (currentIndex === -1) {
      return seasonMonths.slice(0, 4);
    }

    const startIndex = Math.max(0, Math.min(currentIndex, seasonMonths.length - 4));
    return seasonMonths.slice(startIndex, startIndex + 4);
  }, [
    seasonMonths,
    viewMonth,
    viewYear,
    selectedSeason,
    defaultSeason,
    currentYear,
    currentMonth,
    currentDate,
  ]);

  const availableSeasons = getAvailableSeasons();

  if (isLoading) {
    return (
      <Card>
        <CardHeader>
          <Skeleton className="h-6 w-48" />
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
            {[1, 2, 3, 4].map((i) => (
              <Skeleton key={i} className="h-64 w-full" />
            ))}
          </div>
        </CardContent>
      </Card>
    );
  }

  const calendar = calendarData?.calendar || [];
  const statistics = calendarData?.statistics;
  const stock = calendarData?.stock;

  return (
    <Card>
      <CardHeader className="pb-4">
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
          <div className="flex items-center gap-2">
            <CalendarIcon className="h-5 w-5" />
            <CardTitle>{t("tempo.calendar.title")}</CardTitle>
          </div>

          <div className="flex items-center gap-2">
            {/* Season selector */}
            <Select value={selectedSeason} onValueChange={setSelectedSeason}>
              <SelectTrigger className="w-[140px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {availableSeasons.map((season) => (
                  <SelectItem key={season} value={season}>
                    {season}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Navigation - only show for past seasons */}
            {selectedSeason !== defaultSeason && (
              <div className="flex items-center gap-1">
                <Button variant="outline" size="icon" onClick={goToPrevMonth}>
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                <Button variant="outline" size="sm" onClick={goToToday}>
                  {t("tempo.today")}
                </Button>
                <Button variant="outline" size="icon" onClick={goToNextMonth}>
                  <ChevronRight className="h-4 w-4" />
                </Button>
              </div>
            )}
          </div>
        </div>

        {/* Legend */}
        <div className="flex flex-wrap items-center gap-4 text-sm mt-2">
          <div className="flex items-center gap-2">
            <div className="w-4 h-4 rounded bg-blue-500" />
            <span>{t("tempo.colors.blue")}</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-4 h-4 rounded bg-gray-300 dark:bg-gray-500" />
            <span>{t("tempo.colors.white")}</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-4 h-4 rounded bg-red-500" />
            <span>{t("tempo.colors.red")}</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="w-4 h-4 rounded bg-blue-100 dark:bg-blue-950/50 border border-dashed border-blue-300 dark:border-blue-700" />
            <span className="text-muted-foreground">{t("tempo.calendar.prediction")}</span>
          </div>
        </div>
      </CardHeader>

      <CardContent className="space-y-4">
        {/* Calendar grid - responsive based on number of months */}
        <div
          className={`grid gap-4 ${
            visibleMonths.length === 1
              ? "grid-cols-1 max-w-sm mx-auto"
              : visibleMonths.length === 2
                ? "grid-cols-1 sm:grid-cols-2 max-w-2xl mx-auto"
                : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-4"
          }`}
        >
          {visibleMonths.map(({ year, month }) => (
            <MonthCalendar
              key={`${year}-${month}`}
              year={year}
              month={month}
              calendarData={calendar}
            />
          ))}
        </div>

        {/* Statistics */}
        {statistics && stock && (
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 pt-4 border-t">
            <div className="text-center">
              <div className="text-2xl font-bold text-blue-600">{statistics.color_counts.BLUE}</div>
              <div className="text-sm text-muted-foreground">{t("tempo.colors.blue")}</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-gray-600 dark:text-gray-300">
                {statistics.color_counts.WHITE}
                <span className="text-sm font-normal">/{stock.white_total}</span>
              </div>
              <div className="text-sm text-muted-foreground">{t("tempo.colors.white")}</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-red-600">
                {statistics.color_counts.RED}
                <span className="text-sm font-normal">/{stock.red_total}</span>
              </div>
              <div className="text-sm text-muted-foreground">{t("tempo.colors.red")}</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-green-600">
                {statistics.predictions_count}
              </div>
              <div className="text-sm text-muted-foreground">
                {t("tempo.calendar.predictionsCount")}
              </div>
            </div>
          </div>
        )}

        {/* Info */}
        <div className="flex items-start gap-2 text-sm text-muted-foreground bg-muted/50 p-3 rounded-lg">
          <Info className="h-4 w-4 mt-0.5 shrink-0" />
          <p>{t("tempo.calendar.info")}</p>
        </div>
      </CardContent>
    </Card>
  );
}

export default TempoCalendar;
