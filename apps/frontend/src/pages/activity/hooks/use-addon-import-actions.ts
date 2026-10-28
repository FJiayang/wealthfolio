import { subscribeToNavigationUpdates } from "@/addons/addons-runtime-context";
import { getDurableNavItems, type AddonLinkSlot } from "@/addons/contribution-registry";
import type { ActionPaletteItem } from "@/components/action-palette";
import { resolveNavigationIcon } from "@/pages/layouts/navigation/navigation-icons";
import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

export function useAddonImportActions(
  slot: Exclude<AddonLinkSlot, "sidebar">,
): ActionPaletteItem[] {
  const navigate = useNavigate();
  const [links, setLinks] = useState(() => getDurableNavItems(slot));

  useEffect(() => {
    const update = () => setLinks(getDurableNavItems(slot));
    const unsubscribe = subscribeToNavigationUpdates(update);
    update();
    return () => {
      unsubscribe();
    };
  }, [slot]);

  return useMemo(
    () =>
      links.map((link) => ({
        label: link.title,
        icon: ({ className }) => resolveNavigationIcon(link.icon, className ?? ""),
        testId: `addon-import-${link.id}`,
        onClick: () => navigate(link.href),
      })),
    [links, navigate],
  );
}
