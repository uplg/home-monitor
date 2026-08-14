import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Loader2, Rabbit, RefreshCw } from "lucide-react";
import { nabaztagApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { toast } from "@/hooks/use-toast";

/**
 * Discreet one-line Nabaztag status: reachability dot, a word about the
 * Tempo sync, and a quiet re-push button. Renders nothing when no rabbit
 * is configured.
 */
export function NabaztagTile() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const statusQuery = useQuery({
    queryKey: ["nabaztag-status"],
    queryFn: nabaztagApi.status,
    staleTime: 60_000,
    refetchInterval: 120_000,
  });

  const pushMutation = useMutation({
    mutationFn: () => nabaztagApi.pushTempo(),
    onSuccess: (response) => {
      queryClient.invalidateQueries({ queryKey: ["nabaztag-status"] });
      toast({
        title: t("nabaztag.tempoPushed"),
        description: response.message,
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("nabaztag.tempoPushFailed"),
        variant: "destructive",
      });
    },
  });

  const data = statusQuery.data;
  if (!data || !data.config.host) return null;

  const reachable = data.reachable;

  return (
    <div className="flex items-center gap-2 rounded-2xl border border-border/60 bg-card/60 px-3 py-2 text-sm text-muted-foreground">
      <Rabbit className="h-4 w-4 shrink-0" />
      <span
        className={`h-2 w-2 shrink-0 rounded-full ${reachable ? "bg-emerald-500" : "bg-muted-foreground/40"}`}
        aria-hidden
      />
      <span className="truncate">
        {reachable ? t("nabaztag.connected") : t("nabaztag.unreachable")}
        {data.config.tempoEnabled && reachable ? ` · ${t("nabaztag.tempoSync")}` : ""}
      </span>
      <Button
        variant="ghost"
        size="icon"
        className="ml-auto h-7 w-7 shrink-0 rounded-full"
        title={t("nabaztag.pushTempo")}
        onClick={() => pushMutation.mutate()}
        disabled={pushMutation.isPending || !reachable}
      >
        {pushMutation.isPending ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <RefreshCw className="h-3.5 w-3.5" />
        )}
      </Button>
    </div>
  );
}
