// LocaleProvider — in-memory locale state. We deliberately do NOT persist the
// locale to localStorage/sessionStorage (CLAUDE.md §5 forbids browser storage
// in the frontend). The persistent preference lives in the `settings` table
// in SQLite and is hydrated through IPC; until that lands (later contract),
// the app boots in the default locale.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { DEFAULT_LOCALE, LOCALES, translate, type Locale } from "@/lib/i18n";

type LocaleContextValue = {
  locale: Locale;
  setLocale: (next: Locale) => void;
};

const LocaleContext = createContext<LocaleContextValue | null>(null);

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(DEFAULT_LOCALE);

  const setLocale = useCallback((next: Locale) => {
    if (!LOCALES.includes(next)) return;
    setLocaleState(next);
  }, []);

  const value = useMemo(() => ({ locale, setLocale }), [locale, setLocale]);

  // Dev-only affordance: exposes the locale switcher and translator on
  // `window.__cloudsaw_dev` so QA can verify hot-switching and the
  // missing-key fallback without a production UI. Compiled out in
  // release builds via Vite's `import.meta.env.DEV` dead-code elimination.
  //
  // Merges into any existing `__cloudsaw_dev` object instead of
  // overwriting it — PR #64 adds `seedDemoFindings` from App.tsx, and
  // a locale change shouldn't clobber that.
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    const w = window as unknown as { __cloudsaw_dev?: Record<string, unknown> };
    w.__cloudsaw_dev = {
      ...w.__cloudsaw_dev,
      locale,
      setLocale,
      translate,
    };
  }, [locale, setLocale]);

  return (
    <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>
  );
}

export function useLocale(): LocaleContextValue {
  const ctx = useContext(LocaleContext);
  if (!ctx) {
    throw new Error("useLocale must be used inside <LocaleProvider>");
  }
  return ctx;
}
