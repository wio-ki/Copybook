import type { Lang } from './app';
import type { Strings } from './strings';

const MIN = 60_000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

const clockFmt = new Map<Lang, Intl.DateTimeFormat>();
function clock(ms: number, lang: Lang): string {
  let f = clockFmt.get(lang);
  if (!f) {
    f = new Intl.DateTimeFormat(lang, { hour: '2-digit', minute: '2-digit', hour12: false });
    clockFmt.set(lang, f);
  }
  return f.format(ms);
}

/** "刚刚", "12 分钟前", "14:32", "昨天 09:10", "11月3日", "2025/11/3". */
export function relativeTime(ms: number, t: Strings, lang: Lang, now = Date.now()): string {
  const diff = now - ms;
  if (diff < MIN) return t.time.now;
  if (diff < 60 * MIN) return t.time.minutes(Math.floor(diff / MIN));
  const today = startOfDay(now);
  if (ms >= today) return clock(ms, lang);
  if (ms >= today - 86_400_000) return t.time.yesterday(clock(ms, lang));
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(lang, sameYear ? { month: 'short', day: 'numeric' } : { dateStyle: 'short' }).format(ms);
}

/** Full timestamp for the detail view. */
export function absoluteTime(ms: number, lang: Lang): string {
  return new Intl.DateTimeFormat(lang, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(ms);
}

export function formatNumber(n: number, lang: Lang): string {
  return new Intl.NumberFormat(lang).format(n);
}

export function formatBytes(bytes: number, lang: Lang): string {
  const units = ['B', 'KB', 'MB', 'GB'];
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u += 1;
  }
  const digits = u === 0 || v >= 100 ? 0 : 1;
  return `${new Intl.NumberFormat(lang, { maximumFractionDigits: digits }).format(v)} ${units[u]}`;
}

const CJK = /[　-〿㐀-鿿豈-﫿＀-￯]/g;
const CODE_START =
  /^\s*(import|export|const|let|var|function|class|def|fn|pub|use|package|return|if|for|while|SELECT|INSERT|UPDATE|DELETE|CREATE|#include|#!|<\?php|<!DOCTYPE|<html|<div|\{|\[)\b?/im;
const SHELL = /^\s*(\$ |> |PS> |sudo |npm |npx |pnpm |yarn |git |cargo |cd |ls |ssh |scp |curl |wget |docker |kubectl |python |pip |winget |choco |code )/;

/** Decide whether a text preview reads better in the monospace face. */
export function looksLikeCode(text: string, lineCount: number): boolean {
  const s = text.slice(0, 1200);
  if (!s.trim()) return false;
  const cjk = (s.match(CJK) ?? []).length;
  if (cjk / s.length > 0.15) return false;
  if (SHELL.test(s)) return true;
  if (CODE_START.test(s) && /[;{}()=<>]/.test(s)) return true;
  const sym = (s.match(/[{}();=<>[\]$&|]/g) ?? []).length;
  if (lineCount > 1 && sym / s.length > 0.035) return true;
  const indented = s.split('\n').filter((l) => /^( {2,}|\t)\S/.test(l)).length;
  return lineCount > 2 && indented / lineCount > 0.3;
}

/** Trim blank edges, cap blank runs, and remove indentation common to all lines. */
export function tidyPreview(text: string): string {
  const lines = text.replace(/\r\n?/g, '\n').replace(/\t/g, '    ').split('\n');
  while (lines.length && !lines[0].trim()) lines.shift();
  while (lines.length && !lines[lines.length - 1].trim()) lines.pop();
  const indents = lines.filter((l) => l.trim()).map((l) => l.match(/^ */)?.[0].length ?? 0);
  const cut = indents.length ? Math.min(...indents) : 0;
  return lines
    .map((l) => l.slice(cut).trimEnd())
    .join('\n')
    .replace(/\n{3,}/g, '\n\n');
}

export interface UrlParts {
  host: string;
  rest: string;
}

export function splitUrl(raw: string): UrlParts | null {
  try {
    const u = new URL(/^https?:\/\//i.test(raw.trim()) ? raw.trim() : `https://${raw.trim()}`);
    let rest = `${u.pathname === '/' ? '' : u.pathname}${u.search}${u.hash}`;
    try {
      rest = decodeURI(rest);
    } catch {
      /* keep encoded */
    }
    return { host: u.host.replace(/^www\./, ''), rest };
  } catch {
    return null;
  }
}

export function splitPath(raw: string): { dir: string; name: string } {
  const p = raw.trim().replace(/^"(.*)"$/, '$1').replace(/[\\/]+$/, '');
  const i = Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/'));
  return i < 0 ? { dir: '', name: p } : { dir: p.slice(0, i + 1), name: p.slice(i + 1) };
}

export function isCssColor(value: string): boolean {
  return typeof CSS !== 'undefined' && CSS.supports('color', value.trim());
}
