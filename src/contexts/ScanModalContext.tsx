// App-wide ScanModal opener — any component can pop the scanner modal
// without having to thread an `account` + `setScanTarget` state pair
// through props.
//
// Usage:
//
//   import { useScanModal } from "@/contexts/ScanModalContext";
//   const { open } = useScanModal();
//   // No account → user picks one inside the modal:
//   open();
//   // Pre-bound account (existing Accounts.tsx pattern):
//   open(account);
//
// Wired into App.tsx so the modal mounts at the root and survives
// route changes. Existing direct uses of <ScanProgressModal>
// (Accounts.tsx) are NOT affected — those continue to render the
// modal locally with their own onScanFinished callback. The global
// instance fires `scanModalRefreshBus` on completion so any view
// that wants to react can subscribe via a simple event listener.
// PR #48 (Dashboard) + PR #49 (Onboarding) will lean on this.

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import ScanProgressModal from "@/routes/ScanProgress";
import type { Account } from "@/lib/ipc";

type OpenOpts = {
  /** Optional. If provided, the modal skips its account picker and
   *  jumps straight to detection for this account. */
  account?: Account;
  /** Optional. Called once when the scan reaches a terminal state.
   *  The provider always also dispatches the global `scan-finished`
   *  DOM event so views that aren't the caller can refresh. */
  onScanFinished?: () => Promise<void> | void;
};

type ScanModalContextValue = {
  /** Open the scan modal. Either pre-bound (legacy) or pickless. */
  open: (opts?: OpenOpts) => void;
  /** Close programmatically. The modal also closes on Escape /
   *  backdrop click via the underlying Modal component. */
  close: () => void;
  /** Whether the modal is currently mounted + visible. */
  isOpen: boolean;
};

const ScanModalContext = createContext<ScanModalContextValue | null>(null);

/** Hook for consumers to open/close the global scan modal. */
export function useScanModal(): ScanModalContextValue {
  const ctx = useContext(ScanModalContext);
  if (!ctx) {
    throw new Error(
      "useScanModal must be used inside <ScanModalProvider>",
    );
  }
  return ctx;
}

/** Global event name dispatched on document when a scan reaches
 *  terminal state via the global modal. Components that want to
 *  refresh on any scan completion (Dashboard recent-activity card,
 *  Findings list) can listen via:
 *    useEffect(() => {
 *      const handler = () => reload();
 *      document.addEventListener(SCAN_FINISHED_EVENT, handler);
 *      return () => document.removeEventListener(SCAN_FINISHED_EVENT, handler);
 *    }, []);
 *
 *  The existing per-route ScanProgressModal instances (Accounts.tsx)
 *  do NOT fire this event — they call their own onScanFinished
 *  directly. Only the global provider instance fires.
 */
export const SCAN_FINISHED_EVENT = "cloudsaw:scan-finished";

type ProviderState = {
  open: boolean;
  prebound: Account | null;
  onScanFinished: (() => Promise<void> | void) | undefined;
};

export function ScanModalProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<ProviderState>({
    open: false,
    prebound: null,
    onScanFinished: undefined,
  });

  const open = useCallback((opts?: OpenOpts) => {
    setState({
      open: true,
      prebound: opts?.account ?? null,
      onScanFinished: opts?.onScanFinished,
    });
  }, []);

  const close = useCallback(() => {
    setState({ open: false, prebound: null, onScanFinished: undefined });
  }, []);

  const onScanFinished = useCallback(async () => {
    // Caller-specific hook first (if any) so its data refresh
    // resolves before the global broadcast lands.
    if (state.onScanFinished) {
      await state.onScanFinished();
    }
    document.dispatchEvent(new CustomEvent(SCAN_FINISHED_EVENT));
  }, [state.onScanFinished]);

  const value = useMemo(
    () => ({ open, close, isOpen: state.open }),
    [open, close, state.open],
  );

  return (
    <ScanModalContext.Provider value={value}>
      {children}
      {state.open ? (
        <ScanProgressModal
          account={state.prebound}
          onClose={close}
          onScanFinished={onScanFinished}
        />
      ) : null}
    </ScanModalContext.Provider>
  );
}
