import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Textarea } from "@wealthfolio/ui/components/ui/textarea";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { deleteSecret, getSecret, setSecret, type MarketDataProviderSetting } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";

const schema = z.object({
  appKeyId: z.string().trim().min(1),
  privateKey: z.string().trim().min(1),
});
interface Credentials {
  appKeyId: string;
  privateKey: string;
}

export function FutuCredentialsForm({ provider }: { provider: MarketDataProviderSetting }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [visible, setVisible] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const form = useForm<Credentials>({
    resolver: zodResolver(schema),
    defaultValues: { appKeyId: "", privateKey: "" },
  });
  const label = (key: string) => t(`settings:market_data_page.futu.${key}`);

  const reveal = async () => {
    if (visible) {
      setVisible(false);
      return;
    }
    setBusy(true);
    setMessage("");
    try {
      if (!loaded) {
        const secret = await getSecret(provider.id);
        if (secret) {
          const value: unknown = JSON.parse(secret);
          const parsed = schema.safeParse(value);
          if (!parsed.success) throw new Error("Invalid credentials");
          form.reset(parsed.data);
        }
        setLoaded(true);
      }
      setVisible(true);
    } catch {
      setMessage(label("load_error"));
    } finally {
      setBusy(false);
    }
  };

  // Keep credentials out of mutation variables and query caches.
  const persist = async (credentials: Credentials | null) => {
    setBusy(true);
    setMessage("");
    let saved = false;
    try {
      if (credentials) {
        await setSecret(provider.id, JSON.stringify(credentials));
      } else {
        await deleteSecret(provider.id);
      }
      saved = true;
      form.reset({ appKeyId: "", privateKey: "" });
      setVisible(false);
      setLoaded(false);
      setMessage(label(credentials ? "saved" : "deleted"));
    } catch (error) {
      const refreshFailed =
        error instanceof Error &&
        error.message.includes("Credential changed, but market data client refresh failed.");
      setMessage(label(saved || refreshFailed ? "refresh_error" : "save_error"));
    } finally {
      setBusy(false);
      await queryClient.invalidateQueries({ queryKey: [QueryKeys.MARKET_DATA_PROVIDER_SETTINGS] });
    }
  };

  return (
    <form onSubmit={form.handleSubmit((value) => persist(value))} className="space-y-3">
      <p className="text-muted-foreground text-xs">{label("help")}</p>
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={reveal}>
        {label(visible ? "hide" : "show")}
      </Button>
      {visible && (
        <>
          <div className="space-y-1">
            <Label htmlFor="futu-app-key-id">AppKeyId</Label>
            <Input id="futu-app-key-id" autoComplete="off" {...form.register("appKeyId")} />
          </div>
          <div className="space-y-1">
            <Label htmlFor="futu-private-key">{label("private_key")}</Label>
            <Textarea
              id="futu-private-key"
              dir="ltr"
              autoComplete="off"
              spellCheck={false}
              rows={5}
              className="font-mono text-xs"
              {...form.register("privateKey")}
            />
          </div>
          {Object.keys(form.formState.errors).length > 0 && (
            <p role="alert" className="text-destructive text-xs">
              {label("required")}
            </p>
          )}
        </>
      )}
      <div className="flex gap-2">
        <Button type="submit" size="sm" disabled={busy || !visible}>
          {t("settings:common_save")}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={() => persist(null)}
        >
          {label("delete")}
        </Button>
      </div>
      {message && (
        <p role="status" className="text-xs">
          {message}
        </p>
      )}
    </form>
  );
}
