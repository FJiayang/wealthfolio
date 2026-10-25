import i18n from "i18next";
import { useMutation } from "@tanstack/react-query";
import { syncMarketData } from "@/adapters";
import { useToast } from "@wealthfolio/ui/components/ui/use-toast";

export function useSyncMarketDataMutation(refetchAll = false, refetchRecentDays?: number) {
  const { toast } = useToast();

  return useMutation({
    mutationFn: async (assetIds: string[]) => {
      await syncMarketData(assetIds, refetchAll, refetchRecentDays);
    },
    // The command acknowledges admission, not completed writes. The shared
    // portfolio-update-complete listener invalidates once the job finishes.
    onError: (error: Error) => {
      toast({
        title: i18n.t("common:notifications.failed_to_sync_market_data"),
        description: error.message,
        variant: "destructive",
      });
    },
  });
}
