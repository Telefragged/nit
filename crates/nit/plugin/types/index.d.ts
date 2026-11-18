/** The oldest active nit change on the current branch. */
export type Badge = { change: number; subject: string; url: string };

declare module "claude-code" {
  interface PluginState {
    nit: { badge: Badge | null };
  }
}
