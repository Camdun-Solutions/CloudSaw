// `useT` is the only entry point components use to resolve user-facing
// strings. It pulls the current locale from `LocaleProvider` and falls back
// to English for missing keys.

import { useCallback } from "react";

import { translate } from "@/lib/i18n";
import { useLocale } from "@/stores/locale";

export function useT() {
  const { locale } = useLocale();
  return useCallback((key: string) => translate(locale, key), [locale]);
}
