// i18n core. Locale dictionaries are bundled JSON, loaded eagerly because
// CloudSaw ships with a fixed set of launch languages (CLAUDE.md §4.7).
//
// Key lookups fall back to English when a translation is missing — never to
// the raw key or to empty text (Contract 01 edge case).

import en from "@/locales/en.json";
import es from "@/locales/es.json";
import fr from "@/locales/fr.json";
import zh from "@/locales/zh.json";

export const LOCALES = ["en", "es", "fr", "zh"] as const;
export type Locale = (typeof LOCALES)[number];

type Dict = Record<string, string>;

const dictionaries: Record<Locale, Dict> = {
  en: en as Dict,
  es: es as Dict,
  fr: fr as Dict,
  zh: zh as Dict,
};

export const DEFAULT_LOCALE: Locale = "en";

/**
 * Look up `key` in `locale`, falling back to English, then to the key itself
 * as a last resort (only happens if a key is missing from `en` too, which the
 * unit test guards against).
 */
export function translate(locale: Locale, key: string): string {
  const dict = dictionaries[locale] ?? dictionaries[DEFAULT_LOCALE];
  if (key in dict) return dict[key];
  const fallback = dictionaries[DEFAULT_LOCALE];
  if (key in fallback) return fallback[key];
  return key;
}

/** All keys present in the English dictionary, used by tests/dev tooling. */
export function knownKeys(): string[] {
  return Object.keys(dictionaries[DEFAULT_LOCALE]);
}
