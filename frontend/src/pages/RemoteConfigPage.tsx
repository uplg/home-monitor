import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { irApi, type IrBinding } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "@/hooks/use-toast";
import {
  IrBindingEditor,
  summarizeIrAction,
  useIrActionSources,
} from "@/components/devices/IrBindingEditor";
import { IrRemoteMap, remoteKeyLabel } from "@/components/devices/IrRemoteMap";
import { ArrowLeft, Edit2, Plus, Radio, RefreshCw, Repeat, Trash2 } from "lucide-react";

export function RemoteConfigPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const sources = useIrActionSources();

  const [isAddDialogOpen, setIsAddDialogOpen] = useState(false);
  // One shared editor dialog: opened by clicking a key on the remote picture
  // or the edit button of a binding row; works for mapped and unmapped keys.
  const [selectedCode, setSelectedCode] = useState<number | null>(null);

  const { data, isLoading, error } = useQuery({
    queryKey: ["ir-keymap"],
    queryFn: irApi.keymap,
  });

  const keymap = data?.keymap ?? {};
  const bindings: Array<[number, IrBinding]> = Object.entries(keymap)
    .map(([code, binding]): [number, IrBinding] => [Number(code), binding])
    .sort((a, b) => a[0] - b[0]);

  const deleteMutation = useMutation({
    mutationFn: (code: number) => irApi.removeBinding(code),
    onSuccess: (_response, code) => {
      queryClient.invalidateQueries({ queryKey: ["ir-keymap"] });
      toast({
        title: t("remote.deleted"),
        description: t("remote.keyBadge", { code }),
      });
    },
    onError: (mutationError) => {
      toast({
        title: t("common.error"),
        description:
          mutationError instanceof Error ? mutationError.message : t("remote.deleteFailed"),
        variant: "destructive",
      });
    },
  });

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center py-12">
        <p className="text-destructive">{t("remote.loadingError")}</p>
        <Button
          variant="outline"
          className="mt-4"
          onClick={() => queryClient.invalidateQueries({ queryKey: ["ir-keymap"] })}
        >
          <RefreshCw className="mr-2 h-4 w-4" />
          {t("common.retry")}
        </Button>
      </div>
    );
  }

  return (
    <div className="container max-w-4xl mx-auto py-6 px-4 space-y-6">
      {/* Header — stacks on mobile so the add button never overflows */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center">
        <div className="flex min-w-0 flex-1 items-center gap-4">
          <Button variant="ghost" size="icon" onClick={() => navigate("/")} className="shrink-0">
            <ArrowLeft className="h-5 w-5" />
          </Button>
          <div className="min-w-0 flex-1">
            <h1 className="text-2xl font-bold flex items-center gap-2">
              <Radio className="h-6 w-6 shrink-0" />
              {t("remote.title")}
            </h1>
            <p className="text-muted-foreground">{t("remote.subtitle")}</p>
          </div>
        </div>
        <Dialog open={isAddDialogOpen} onOpenChange={setIsAddDialogOpen}>
          <DialogTrigger asChild>
            <Button className="w-full sm:w-auto">
              <Plus className="mr-2 h-4 w-4" />
              {t("remote.addBinding")}
            </Button>
          </DialogTrigger>
          <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <Plus className="h-5 w-5" />
                {t("remote.addBinding")}
              </DialogTitle>
              <DialogDescription>{t("remote.editorDescription")}</DialogDescription>
            </DialogHeader>
            <IrBindingEditor
              keymap={keymap}
              onSaved={() => setIsAddDialogOpen(false)}
              onCancel={() => setIsAddDialogOpen(false)}
            />
          </DialogContent>
        </Dialog>
      </div>

      {/* Remote picture + bindings */}
      <div className="grid items-start gap-6 lg:grid-cols-[auto_1fr]">
        <IrRemoteMap keymap={keymap} onSelect={(code) => setSelectedCode(code)} />

        {isLoading ? (
          <div className="space-y-3">
            <Skeleton className="h-24" />
            <Skeleton className="h-24" />
            <Skeleton className="h-24" />
          </div>
        ) : bindings.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <Radio className="h-12 w-12 text-muted-foreground mb-4" />
              <p className="text-muted-foreground text-center">{t("remote.noBindings")}</p>
              <p className="text-sm text-muted-foreground text-center mt-1">
                {t("remote.noBindingsHint")}
              </p>
            </CardContent>
          </Card>
        ) : (
          <div className="space-y-3">
            {bindings.map(([code, binding]) => (
              <Card key={code} className="transition-all hover:shadow-md">
                <CardContent className="p-3 sm:p-4">
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:gap-4">
                    <div className="flex h-12 w-full sm:h-16 sm:w-16 flex-row sm:flex-col items-center justify-center gap-2 sm:gap-0 rounded-xl bg-primary/10 text-primary">
                      <span className="text-base sm:text-lg font-bold">
                        {remoteKeyLabel(code) ?? code}
                      </span>
                      <span className="font-mono text-[10px] opacity-60">#{code}</span>
                    </div>

                    <div className="flex-1 min-w-0">
                      <div className="flex flex-wrap items-center gap-2 mb-1">
                        <span className="font-medium truncate">
                          {binding.label || t("remote.keyBadge", { code })}
                        </span>
                        {binding.repeat && (
                          <Badge variant="outline">
                            <Repeat className="mr-1 h-3 w-3" />
                            {t("remote.repeat")}
                          </Badge>
                        )}
                      </div>
                      <div className="space-y-0.5">
                        {binding.actions.map((action, index) => (
                          <p key={index} className="text-sm text-muted-foreground truncate">
                            {summarizeIrAction(action, sources, t)}
                          </p>
                        ))}
                      </div>
                    </div>

                    <div className="flex items-center justify-end gap-1 border-t pt-3 sm:border-0 sm:pt-0">
                      <Button variant="ghost" size="icon" onClick={() => setSelectedCode(code)}>
                        <Edit2 className="h-4 w-4" />
                      </Button>

                      <Button
                        variant="ghost"
                        size="icon"
                        className="text-destructive hover:text-destructive"
                        onClick={() => deleteMutation.mutate(code)}
                        disabled={deleteMutation.isPending}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        )}
      </div>

      {/* Shared editor dialog, opened from the remote picture or a row */}
      <Dialog open={selectedCode !== null} onOpenChange={(open) => !open && setSelectedCode(null)}>
        <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg">
          {selectedCode !== null && (
            <>
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2">
                  {keymap[String(selectedCode)] ? (
                    <>
                      <Edit2 className="h-5 w-5" />
                      {t("remote.editBinding", { code: selectedCode })}
                    </>
                  ) : (
                    <>
                      <Plus className="h-5 w-5" />
                      {t("remote.addBinding")}
                    </>
                  )}
                </DialogTitle>
                <DialogDescription>{t("remote.editorDescription")}</DialogDescription>
              </DialogHeader>
              <IrBindingEditor
                initialCode={selectedCode}
                keymap={keymap}
                onSaved={() => setSelectedCode(null)}
                onCancel={() => setSelectedCode(null)}
              />
            </>
          )}
        </DialogContent>
      </Dialog>

      {bindings.length > 0 && (
        <div className="text-center text-sm text-muted-foreground">
          {t("remote.bindingCount", { count: bindings.length })}
        </div>
      )}
    </div>
  );
}
