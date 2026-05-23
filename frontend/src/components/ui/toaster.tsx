import { Toaster as Sonner } from "sonner";

export function Toaster() {
  return (
    <Sonner
      position="top-right"
      richColors
      closeButton
      toastOptions={{
        classNames: {
          toast: "border border-border bg-card text-card-foreground shadow-lg",
          title: "text-sm font-medium text-foreground",
          description: "text-sm text-muted-foreground",
          actionButton: "bg-primary text-primary-foreground",
          cancelButton: "bg-muted text-muted-foreground",
          error: "border-red-200 bg-red-50 text-red-950",
          success: "border-emerald-200 bg-emerald-50 text-emerald-950",
        },
      }}
    />
  );
}
