const guards = new Map<string, boolean>();
const listeners = new Set<() => void>();

export function setAddonLeaveGuard(addonId: string, enabled: boolean) {
  if (enabled) guards.set(addonId, true);
  else guards.delete(addonId);
  listeners.forEach((listener) => listener());
}
export function getAddonLeaveGuard(addonId: string) {
  return guards.has(addonId);
}
export function subscribeAddonLeaveGuard(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
