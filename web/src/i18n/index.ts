import { useSyncExternalStore } from 'react';
import { en } from './en';
import { de } from './de';
import { es } from './es';
import { fr } from './fr';
import { id } from './id';
import { ja } from './ja';
import { ko } from './ko';
import { ru } from './ru';
import { vi } from './vi';
import { zh } from './zh';

export type Key = keyof typeof en;
export type Dict = Record<Key, string>;

export const LANGS = ['vi', 'en', 'zh', 'ja', 'ko', 'id', 'es', 'fr', 'de', 'ru'] as const;
export type Lang = (typeof LANGS)[number];

/** Tên mỗi ngôn ngữ viết bằng chính ngôn ngữ đó. */
export const NAMES: Record<Lang, string> = {
  vi: 'Tiếng Việt',
  en: 'English',
  zh: '中文 (简体)',
  ja: '日本語',
  ko: '한국어',
  id: 'Bahasa Indonesia',
  es: 'Español',
  fr: 'Français',
  de: 'Deutsch',
  ru: 'Русский',
};

const DICTS: Record<Lang, Dict> = { vi, en, zh, ja, ko, id, es, fr, de, ru };
const STORE_KEY = 'larma-lang';

const isLang = (s: unknown): s is Lang => typeof s === 'string' && (LANGS as readonly string[]).includes(s);

/** Mã ngôn ngữ của trình duyệt ("zh-CN", "in-ID"...) đổi sang ngôn ngữ có sẵn, không có thì English. */
export function fromLocale(tag: string): Lang {
  const p = tag.toLowerCase().split(/[-_]/)[0];
  const c = p === 'in' ? 'id' : p;
  return isLang(c) ? c : 'en';
}

function detect(): Lang {
  const list = navigator.languages?.length ? navigator.languages : [navigator.language];
  for (const tag of list) {
    const l = fromLocale(tag ?? '');
    if (l !== 'en' || (tag ?? '').toLowerCase().startsWith('en')) return l;
  }
  return 'en';
}

function readOverride(): Lang | null {
  try {
    const v = localStorage.getItem(STORE_KEY);
    return isLang(v) ? v : null;
  } catch {
    return null;
  }
}

/** Người dùng tự chọn trên điện thoại (null = để theo PC). */
let override: Lang | null = readOverride();
/** Ngôn ngữ PC báo về trong tin "state". */
let pcLang: Lang | null = null;
let current: Lang = override ?? detect();
const listeners = new Set<() => void>();

function apply() {
  const next = override ?? pcLang ?? detect();
  if (next === current && document.documentElement.lang === next) return;
  current = next;
  document.documentElement.lang = next;
  listeners.forEach((f) => f());
}

document.documentElement.lang = current;

export function setLang(l: Lang | 'auto') {
  override = l === 'auto' ? null : l;
  try {
    if (override) localStorage.setItem(STORE_KEY, override);
    else localStorage.removeItem(STORE_KEY);
  } catch {
    /* không lưu được thì thôi */
  }
  apply();
}

/** Khi chưa tự chọn, điện thoại theo ngôn ngữ đang dùng trên PC. */
export function adoptPcLang(code: unknown) {
  if (!isLang(code) || code === pcLang) return;
  pcLang = code;
  apply();
}

/** Lựa chọn của người dùng ('auto' = theo PC/trình duyệt). */
export function chosen(): Lang | 'auto' {
  return override ?? 'auto';
}

const subscribe = (f: () => void) => {
  listeners.add(f);
  return () => {
    listeners.delete(f);
  };
};

/** Gọi trong component để vẽ lại khi đổi ngôn ngữ. */
export function useLang(): Lang {
  return useSyncExternalStore(subscribe, () => current);
}

/** Chữ theo ngôn ngữ hiện tại; thay {0}, {1}… bằng tham số. Thiếu thì lùi về English. */
export function t(key: Key, ...args: (string | number)[]): string {
  let s = DICTS[current][key] ?? en[key];
  args.forEach((a, i) => {
    s = s.split(`{${i}}`).join(String(a));
  });
  return s;
}
