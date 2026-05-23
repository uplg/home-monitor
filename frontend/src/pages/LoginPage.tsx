import { useState } from "react";
import { Navigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useAuth } from "@/contexts/AuthContext";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { toast } from "@/hooks/use-toast";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { House, Loader2 } from "lucide-react";

export function LoginPage() {
  const { t } = useTranslation();
  const { isAuthenticated, login } = useAuth();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  if (isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsLoading(true);

    try {
      await login(username, password);
      toast({
        title: t("auth.loginSuccess"),
        description: t("auth.loginSuccessDescription"),
      });
    } catch (error) {
      toast({
        title: t("auth.loginError"),
        description: error instanceof Error ? error.message : t("auth.invalidCredentials"),
        variant: "destructive",
      });
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="relative flex min-h-screen items-center justify-center overflow-hidden p-4">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_top_left,hsl(193_77%_86%/.7),transparent_30%),radial-gradient(circle_at_85%_18%,hsl(36_100%_86%/.6),transparent_24%),linear-gradient(180deg,hsl(38_60%_98%),hsl(210_45%_98%))]" />
      <div className="absolute top-4 right-4">
        <LanguageSwitcher />
      </div>
      <Card className="relative w-full max-w-md border-border bg-card/88 shadow-[0_32px_100px_-36px_rgba(15,23,42,0.35)] backdrop-blur">
        <CardHeader className="space-y-4 px-8 pt-8 text-center">
          <div className="mx-auto flex h-14 w-14 items-center justify-center rounded-2xl bg-primary text-primary-foreground shadow-lg shadow-slate-900/15">
            <House className="h-7 w-7" />
          </div>
          <div className="space-y-2">
            <CardTitle className="text-3xl tracking-[-0.04em] text-foreground">
              {t("branding.name")}
            </CardTitle>
            <CardDescription className="text-sm text-muted-foreground">
              {t("auth.loginDescription")}
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="px-8 pb-8">
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">{t("auth.username")}</Label>
              <Input
                id="username"
                type="text"
                placeholder="admin"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                required
                autoComplete="username"
                disabled={isLoading}
                className="h-11 border-border bg-muted/70"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">{t("auth.password")}</Label>
              <Input
                id="password"
                type="password"
                placeholder="••••••••"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                autoComplete="current-password"
                disabled={isLoading}
                className="h-11 border-border bg-muted/70"
              />
            </div>
            <Button
              type="submit"
              className="h-11 w-full bg-primary text-primary-foreground hover:bg-primary/90"
              disabled={isLoading}
            >
              {isLoading ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t("auth.loggingIn")}
                </>
              ) : (
                t("auth.login")
              )}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
