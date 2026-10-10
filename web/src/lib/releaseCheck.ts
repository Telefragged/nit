import { useCallback, useEffect, useState } from "react";
import { readSaved, writeSaved } from "./storage";

const KEY = "nit.release-check";
const HOUR = 60 * 60 * 1000;

/** The release check, shared by every tab of this browser. */
interface ReleaseCheck {
  /** Off until the reviewer turns it on, since each check tells GitHub
   * the reviewer's IP address. */
  enabled: boolean;
  /** When a tab last asked GitHub, in epoch milliseconds. */
  checkedAt: number;
  /** The tag of the latest release, or `null` before the first answer. */
  latest: string | null;
}

function load(): ReleaseCheck {
  const saved = readSaved(KEY) as Partial<ReleaseCheck> | null;
  return {
    enabled: saved?.enabled === true,
    checkedAt: typeof saved?.checkedAt === "number" ? saved.checkedAt : 0,
    latest: typeof saved?.latest === "string" ? saved.latest : null,
  };
}

/** Whether release `latest` is newer than `current`.
 *
 * Both are semver. A leading `v`, a pre-release and a build suffix
 * (`+<sha>`) are ignored, so a build of a released commit is not behind. */
export function isNewer(latest: string, current: string): boolean {
  const parse = (version: string) =>
    (version.replace(/^v/, "").split(/[+-]/)[0] ?? "").split(".").map(Number);
  const [a, b] = [parse(latest), parse(current)];
  for (let i = 0; i < 3; i++) {
    const [x, y] = [a[i] ?? 0, b[i] ?? 0];
    if (x !== y) return x > y;
  }
  return false;
}

/** Returns the tag of nit's latest release on GitHub. */
async function getLatestRelease(): Promise<string> {
  if (import.meta.env.VITE_MOCK) return "v0.2.0";
  const res = await fetch(
    "https://api.github.com/repos/Telefragged/nit/releases/latest",
  );
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return ((await res.json()) as { tag_name: string }).tag_name;
}

/** Asks GitHub for the latest release and saves the answer.
 *
 * A failed request keeps the last answer but still counts as a check, so
 * an offline browser does not retry on every focus. */
async function ask(): Promise<ReleaseCheck> {
  const latest = await getLatestRelease().catch(() => load().latest);
  const next = { ...load(), checkedAt: Date.now(), latest };
  writeSaved(KEY, next);
  return next;
}

/** The release check, a toggle that turns it on or off, and a refresh
 * that asks GitHub now.
 *
 * While enabled, the tabs of this browser ask GitHub at most once an
 * hour between them, and each tab shows the latest answer. */
export function useReleaseCheck(): {
  check: ReleaseCheck;
  setEnabled: (enabled: boolean) => void;
  refresh: () => Promise<void>;
} {
  const [check, setCheck] = useState(load);

  useEffect(() => {
    // Another tab's toggle or answer arrives as a storage event.
    const onStorage = (e: StorageEvent) => {
      if (e.key === KEY) setCheck(load());
    };
    addEventListener("storage", onStorage);
    return () => {
      removeEventListener("storage", onStorage);
    };
  }, []);

  useEffect(() => {
    if (!check.enabled) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    // Runs under a lock shared by the tabs, so two tabs that wake at once
    // ask GitHub once. Returns when the next check is due.
    const checkOnce = async (): Promise<number> => {
      const saved = load();
      if (!saved.enabled || Date.now() - saved.checkedAt < HOUR) {
        return saved.checkedAt + HOUR;
      }
      const next = await ask();
      setCheck(next);
      return next.checkedAt + HOUR;
    };

    const run = async () => {
      clearTimeout(timer);
      // Web Locks exist only in a secure context, and nit may be served
      // over plain http on a LAN. That falls back to one check per tab.
      const due =
        "locks" in navigator
          ? await navigator.locks.request(KEY, checkOnce)
          : await checkOnce();
      // No event fires when an hour has passed, so a visible tab sets a
      // timer for the next check. A hidden tab checks when it is shown.
      if (!cancelled && document.visibilityState === "visible") {
        timer = setTimeout(() => void run(), due - Date.now());
      }
    };

    const onVisibility = () => {
      if (document.visibilityState === "visible") void run();
      else clearTimeout(timer);
    };
    void run();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      cancelled = true;
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [check.enabled]);

  const setEnabled = useCallback((enabled: boolean) => {
    const next = { ...load(), enabled };
    writeSaved(KEY, next);
    setCheck(next);
  }, []);
  const refresh = useCallback(async () => {
    setCheck(await ask());
  }, []);
  return { check, setEnabled, refresh };
}
