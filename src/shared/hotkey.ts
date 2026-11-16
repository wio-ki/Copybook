// Hotkey strings per docs/contract.md: "Ctrl+Alt+Shift+Super+<KeyboardEvent.code>".

const MODIFIER_CODES = new Set([
  'ControlLeft',
  'ControlRight',
  'AltLeft',
  'AltRight',
  'ShiftLeft',
  'ShiftRight',
  'MetaLeft',
  'MetaRight',
  'OSLeft',
  'OSRight',
]);

const LABELS: Record<string, string> = {
  Super: 'Win',
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Space: 'Space',
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Enter: 'Enter',
  Escape: 'Esc',
  Backspace: 'Backspace',
  Delete: 'Del',
  Insert: 'Ins',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
};

function keyLabel(token: string): string {
  if (token in LABELS) return LABELS[token];
  if (/^Key[A-Z]$/.test(token)) return token.slice(3);
  if (/^Digit\d$/.test(token)) return token.slice(5);
  if (/^Numpad\d$/.test(token)) return `Num ${token.slice(6)}`;
  return token;
}

/** "Alt+KeyV" -> ["Alt", "V"] for key caps. */
export function hotkeyParts(hotkey: string): string[] {
  if (!hotkey) return [];
  return hotkey.split('+').map(keyLabel);
}

/** "Alt+KeyV" -> "Alt+V" for running text. */
export function hotkeyText(hotkey: string): string {
  return hotkeyParts(hotkey).join('+');
}

export type RecordResult =
  | { kind: 'pending'; parts: string[] }
  | { kind: 'done'; hotkey: string }
  | { kind: 'needs-modifier' };

/** Turn a keydown into a hotkey string, or tell the recorder what is still missing. */
export function hotkeyFromEvent(e: KeyboardEvent): RecordResult {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey) mods.push('Super');
  if (MODIFIER_CODES.has(e.code) || !e.code) return { kind: 'pending', parts: mods.map(keyLabel) };
  const isFn = /^F([1-9]|1\d|2[0-4])$/.test(e.code);
  if (!mods.length && !isFn) return { kind: 'needs-modifier' };
  return { kind: 'done', hotkey: [...mods, e.code].join('+') };
}
