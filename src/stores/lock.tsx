// LockProvider — owns the app's notion of "are we locked right now?" and the
// snapshot of lock settings the rest of the UI reads.
//
// The store is a thin wrapper around the `applock_*` IPC commands. Truth lives
// in Rust; this provider just caches the latest state and re-fetches after any
// mutation so screens stay in sync without bespoke event plumbing.
//
// No values touched here are persisted in browser storage (CLAUDE.md §5
// forbids localStorage/sessionStorage). The session-lifetime cache is
// regular React state; long-lived state is in SQLite + Rust memory.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { ipc, type LockState } from "@/lib/ipc";

type Status = "loading" | "ready" | "error";

type LockContextValue = {
  status: Status;
  state: LockState | null;
  error: string | null;
  refresh: () => Promise<void>;
};

const LockContext = createContext<LockContextValue | null>(null);

export function LockProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<LockState | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await ipc.applockGetState();
      setState(next);
      setStatus("ready");
      setError(null);
    } catch (err) {
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: unknown }).message)
          : "Failed to read app-lock state.";
      setError(msg);
      setStatus("error");
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const value = useMemo<LockContextValue>(
    () => ({ status, state, error, refresh }),
    [status, state, error, refresh],
  );

  return <LockContext.Provider value={value}>{children}</LockContext.Provider>;
}

export function useLock(): LockContextValue {
  const ctx = useContext(LockContext);
  if (!ctx) throw new Error("useLock must be used inside <LockProvider>");
  return ctx;
}
