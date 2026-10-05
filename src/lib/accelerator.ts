// Mute-hotkey recording and display. An accelerator is the string Tauri's global-shortcut
// plugin parses ("CommandOrControl+Alt+Shift+M"): modifiers, then exactly one key, joined by
// "+". Recording reads the modifier flags and the key of one key press, builds the accelerator
// and keeps nothing else.
//
// What a key token means depends on how the plugin registers it on each OS:
// - macOS: a key position (a virtual key code, named after the US layout). "M" is the key
//   right of N on a US keyboard, which types "," on French AZERTY. The recorder uses
//   `KeyboardEvent.code`, the same position. Labels show the layout's character for that
//   position when the app (`key_labels`, UCKeyTranslate) or the webview can tell
//   (keyboard.svelte.ts); otherwise they name the US key and say so (`namesUsPosition`).
// - Windows: a virtual-key code, which follows the layout ("M" is the key that types M). The
//   recorder takes letters, digits and punctuation from the press's virtual-key code
//   (`keyCode`), so the key pressed is the key registered.
// - Linux (X11): the key that types the token's character; letters come from `keyCode` too.

import type { Platform } from './platform';

type Mod = 'ctrl' | 'alt' | 'shift' | 'super';

interface KeyName {
  /** The accelerator token (accepted by the plugin's parser). */
  token: string;
  /** Shown on macOS. */
  mac: string;
  /** Shown on Windows and Linux. */
  other: string;
}

/** The modifier flags and key of one key press (a subset of `KeyboardEvent`). */
export interface KeyPress {
  /** The physical key, named after its place on a US keyboard. */
  code: string;
  /**
   * The legacy key code. Chromium on Windows (WebView2) reports the Windows virtual-key code,
   * which the layout assigns; WebKitGTK derives it from the typed character.
   */
  keyCode?: number;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** The current layout's character per `KeyboardEvent.code` (the Keyboard Map API's map). */
export type KeyLabels = ReadonlyMap<string, string>;

export type RecordResult =
  /** Only modifiers are down so far; `held` shows them (e.g. "⌥⇧"). */
  | { kind: 'modifiers'; held: string }
  /** Escape without modifiers: stop recording. */
  | { kind: 'cancel' }
  /** The key can't be used; `reason` says why. */
  | { kind: 'invalid'; reason: string }
  | { kind: 'accelerator'; accelerator: string };

const KEYS = new Map<string, KeyName>();

function add(code: string, token: string, mac: string, other = mac): void {
  KEYS.set(code, { token, mac, other });
}

for (let c = 65; c <= 90; c++) {
  const letter = String.fromCharCode(c);
  add(`Key${letter}`, letter, letter);
}
for (let d = 0; d <= 9; d++) {
  add(`Digit${d}`, `${d}`, `${d}`);
  add(`Numpad${d}`, `Numpad${d}`, `Num${d}`);
}
for (let f = 1; f <= 24; f++) add(`F${f}`, `F${f}`, `F${f}`);
add('Space', 'Space', 'Space');
add('Enter', 'Enter', '↩', 'Enter');
add('Tab', 'Tab', '⇥', 'Tab');
add('Backspace', 'Backspace', '⌫', 'Backspace');
add('Delete', 'Delete', '⌦', 'Del');
add('Escape', 'Escape', '⎋', 'Esc');
add('ArrowUp', 'ArrowUp', '↑', 'Up');
add('ArrowDown', 'ArrowDown', '↓', 'Down');
add('ArrowLeft', 'ArrowLeft', '←', 'Left');
add('ArrowRight', 'ArrowRight', '→', 'Right');
add('Home', 'Home', '↖', 'Home');
add('End', 'End', '↘', 'End');
add('PageUp', 'PageUp', '⇞', 'PgUp');
add('PageDown', 'PageDown', '⇟', 'PgDn');
add('Insert', 'Insert', 'Ins');
add('PrintScreen', 'PrintScreen', 'PrtSc');
add('ScrollLock', 'ScrollLock', 'ScrLk');
add('Pause', 'Pause', 'Pause');
add('Minus', 'Minus', '-');
add('Equal', 'Equal', '=');
add('BracketLeft', 'BracketLeft', '[');
add('BracketRight', 'BracketRight', ']');
add('Backslash', 'Backslash', '\\');
add('Semicolon', 'Semicolon', ';');
add('Quote', 'Quote', "'");
add('Comma', 'Comma', ',');
add('Period', 'Period', '.');
add('Slash', 'Slash', '/');
add('Backquote', 'Backquote', '`');
add('NumpadAdd', 'NumpadAdd', 'Num+');
add('NumpadSubtract', 'NumpadSubtract', 'Num-');
add('NumpadMultiply', 'NumpadMultiply', 'Num*');
add('NumpadDivide', 'NumpadDivide', 'Num/');
add('NumpadDecimal', 'NumpadDecimal', 'Num.');
add('NumpadEnter', 'NumpadEnter', 'Num↩', 'NumEnter');
add('NumpadEqual', 'NumpadEqual', 'Num=');

/** Punctuation keys: like letters and digits, what they type depends on the layout. */
const PUNCTUATION_CODES = new Set([
  'Minus',
  'Equal',
  'BracketLeft',
  'BracketRight',
  'Backslash',
  'Semicolon',
  'Quote',
  'Comma',
  'Period',
  'Slash',
  'Backquote',
]);

/** True for the keys whose character depends on the layout: letters, digits, punctuation. */
function isCharacterKey(code: string): boolean {
  return /^(Key[A-Z]|Digit\d)$/.test(code) || PUNCTUATION_CODES.has(code);
}

/** F1–F24: they work as a hotkey on their own (as in hotkey.rs). */
function isFunctionKey(code: string): boolean {
  return /^F([1-9]|1\d|2[0-4])$/.test(code);
}

/**
 * Windows virtual-key codes of character keys → the code whose token global-hotkey registers
 * as that same virtual key (its `key_to_vk`: KeyM → VK_M, Semicolon → VK_OEM_1, …).
 */
const VK_TO_CODE = new Map<number, string>([
  [186, 'Semicolon'],
  [187, 'Equal'],
  [188, 'Comma'],
  [189, 'Minus'],
  [190, 'Period'],
  [191, 'Slash'],
  [192, 'Backquote'],
  [219, 'BracketLeft'],
  [220, 'Backslash'],
  [221, 'BracketRight'],
  [222, 'Quote'],
]);
for (let c = 65; c <= 90; c++) VK_TO_CODE.set(c, `Key${String.fromCharCode(c)}`);
for (let d = 0; d <= 9; d++) VK_TO_CODE.set(48 + d, `Digit${d}`);

/** Every spelling the plugin accepts for a key (upper case) → the `KeyboardEvent.code`. */
const ALIASES = new Map<string, string>();
for (const [code, name] of KEYS) {
  ALIASES.set(code.toUpperCase(), code);
  ALIASES.set(name.token.toUpperCase(), code);
}
for (const [alias, code] of Object.entries({
  ESC: 'Escape',
  UP: 'ArrowUp',
  DOWN: 'ArrowDown',
  LEFT: 'ArrowLeft',
  RIGHT: 'ArrowRight',
  PAUSEBREAK: 'Pause',
  '-': 'Minus',
  '=': 'Equal',
  '[': 'BracketLeft',
  ']': 'BracketRight',
  '\\': 'Backslash',
  ';': 'Semicolon',
  "'": 'Quote',
  ',': 'Comma',
  '.': 'Period',
  '/': 'Slash',
  '`': 'Backquote',
  NUMADD: 'NumpadAdd',
  NUMPADPLUS: 'NumpadAdd',
  NUMPLUS: 'NumpadAdd',
  NUMSUBTRACT: 'NumpadSubtract',
  NUMMULTIPLY: 'NumpadMultiply',
  NUMDIVIDE: 'NumpadDivide',
  NUMDECIMAL: 'NumpadDecimal',
  NUMENTER: 'NumpadEnter',
  NUMEQUAL: 'NumpadEqual',
})) {
  ALIASES.set(alias, code);
}
for (let d = 0; d <= 9; d++) ALIASES.set(`NUM${d}`, `Numpad${d}`);

/** Codes of keys that only modify (never the shortcut's key). */
const MODIFIER_CODES = new Set([
  'ShiftLeft',
  'ShiftRight',
  'ControlLeft',
  'ControlRight',
  'AltLeft',
  'AltRight',
  'MetaLeft',
  'MetaRight',
  'OSLeft',
  'OSRight',
  'CapsLock',
  'Fn',
  'FnLock',
]);

const MOD_ORDER: Mod[] = ['ctrl', 'alt', 'shift', 'super'];

const MOD_SYMBOL: Record<Platform, Record<Mod, string>> = {
  mac: { ctrl: '⌃', alt: '⌥', shift: '⇧', super: '⌘' },
  windows: { ctrl: 'Ctrl', alt: 'Alt', shift: 'Shift', super: 'Win' },
  linux: { ctrl: 'Ctrl', alt: 'Alt', shift: 'Shift', super: 'Super' },
};

const MOD_WORD: Record<Platform, Record<Mod, string>> = {
  mac: { ctrl: 'Control', alt: 'Option', shift: 'Shift', super: 'Command' },
  windows: { ctrl: 'Control', alt: 'Alt', shift: 'Shift', super: 'Windows' },
  linux: { ctrl: 'Control', alt: 'Alt', shift: 'Shift', super: 'Super' },
};

/** The modifier an accelerator token stands for on `p`, or null if it is not a modifier. */
function modifierOf(token: string, p: Platform): Mod | null {
  switch (token.toUpperCase()) {
    case 'COMMANDORCONTROL':
    case 'COMMANDORCTRL':
    case 'CMDORCTRL':
    case 'CMDORCONTROL':
      return p === 'mac' ? 'super' : 'ctrl';
    case 'COMMAND':
    case 'CMD':
    case 'SUPER':
      return 'super';
    case 'CONTROL':
    case 'CTRL':
      return 'ctrl';
    case 'ALT':
    case 'OPTION':
      return 'alt';
    case 'SHIFT':
      return 'shift';
    default:
      return null;
  }
}

interface Parsed {
  mods: Set<Mod>;
  /** The key's `KeyboardEvent.code`, or the raw token if it is not one we know. */
  key: string | null;
}

function parse(accelerator: string, p: Platform): Parsed {
  const mods = new Set<Mod>();
  let key: string | null = null;
  for (const raw of accelerator.split('+')) {
    const token = raw.trim();
    if (!token) continue;
    const mod = modifierOf(token, p);
    if (mod) mods.add(mod);
    else key = ALIASES.get(token.toUpperCase()) ?? token;
  }
  return { mods, key };
}

/**
 * The character the layout puts on a key position, for macOS labels (elsewhere a token names
 * the character already). Null when unknown or not a single visible character (a dead key).
 */
function layoutCharacter(key: string, p: Platform, layout?: KeyLabels | null): string | null {
  if (p !== 'mac' || !layout || !isCharacterKey(key)) return null;
  const char = layout.get(key);
  if (!char || [...char].length !== 1 || !char.trim()) return null;
  return char.toUpperCase();
}

function keyLabel(key: string, p: Platform, layout?: KeyLabels | null): string {
  const char = layoutCharacter(key, p, layout);
  if (char) return char;
  const name = KEYS.get(key);
  if (!name) return key;
  return p === 'mac' ? name.mac : name.other;
}

/**
 * The accelerator as keycap labels in the platform's order: `["⌥", "⇧", "⌘", "M"]` on macOS
 * (Apple's ⌃⌥⇧⌘ order), `["Ctrl", "Alt", "Shift", "M"]` elsewhere. On macOS, `layout` puts
 * the layout's character on letter, digit and punctuation keys.
 */
export function acceleratorParts(
  accelerator: string,
  p: Platform,
  layout?: KeyLabels | null,
): string[] {
  const { mods, key } = parse(accelerator, p);
  const parts = MOD_ORDER.filter((m) => mods.has(m)).map((m) => MOD_SYMBOL[p][m]);
  if (key) parts.push(keyLabel(key, p, layout));
  return parts;
}

/** The accelerator as one string: "⌥⇧⌘M" on macOS, "Ctrl+Alt+Shift+M" elsewhere. */
export function formatAccelerator(
  accelerator: string,
  p: Platform,
  layout?: KeyLabels | null,
): string {
  return acceleratorParts(accelerator, p, layout).join(p === 'mac' ? '' : '+');
}

/** Words for screen readers: "Option Shift Command M". */
export function describeAccelerator(
  accelerator: string,
  p: Platform,
  layout?: KeyLabels | null,
): string {
  const { mods, key } = parse(accelerator, p);
  const words = MOD_ORDER.filter((m) => mods.has(m)).map((m) => MOD_WORD[p][m]);
  if (key) words.push(layoutCharacter(key, p, layout) ?? KEYS.get(key)?.token ?? key);
  return words.join(' ');
}

/**
 * True when the label of the accelerator's key names a US key position the user's layout may
 * label differently: macOS (positional hotkeys), a letter, digit or punctuation key, and no
 * layout map (neither the app's `key_labels` nor the webview's Keyboard Map API).
 */
export function namesUsPosition(
  accelerator: string,
  p: Platform,
  layout?: KeyLabels | null,
): boolean {
  if (p !== 'mac' || layout) return false;
  const { key } = parse(accelerator, p);
  return key !== null && isCharacterKey(key);
}

function heldMods(e: KeyPress): Set<Mod> {
  const mods = new Set<Mod>();
  if (e.ctrlKey) mods.add('ctrl');
  if (e.altKey) mods.add('alt');
  if (e.shiftKey) mods.add('shift');
  if (e.metaKey) mods.add('super');
  return mods;
}

/** The modifiers currently held, for live feedback while recording ("⌥⇧" / "Alt+Shift"). */
export function formatHeld(e: KeyPress, p: Platform): string {
  const mods = heldMods(e);
  return MOD_ORDER.filter((m) => mods.has(m))
    .map((m) => MOD_SYMBOL[p][m])
    .join(p === 'mac' ? '' : '+');
}

/** The modifiers a shortcut needs besides Shift, named for the hint. */
export function needModifierHint(p: Platform): string {
  const need =
    p === 'mac'
      ? 'Add ⌘, ⌥ or ⌃: ⇧ alone would get in the way of typing.'
      : `Add Ctrl, Alt or ${MOD_SYMBOL[p].super}: Shift alone would get in the way of typing.`;
  return `${need} Only F1–F24 work alone.`;
}

/**
 * The code of the key the plugin should register for a press: its position on macOS; on
 * Windows the key with the press's virtual-key code; on Linux the letter it types. WebKitGTK
 * derives `keyCode` from the shifted character, which differs for digits and punctuation
 * (AZERTY's "," key gives "?"), so Linux takes only letters from it.
 */
function registeredCode(e: KeyPress, p: Platform): string {
  if (p === 'mac' || e.keyCode === undefined || !isCharacterKey(e.code)) return e.code;
  const code = VK_TO_CODE.get(e.keyCode);
  if (!code || (p === 'linux' && !code.startsWith('Key'))) return e.code;
  return code;
}

/**
 * Turns one key press into an accelerator. Except for F1–F24, the shortcut needs a modifier
 * other than Shift (a global Shift+M would swallow every capital M), and it needs one supported
 * key. Cmd on macOS and Ctrl elsewhere become `CommandOrControl`, like the default shortcut.
 */
export function recordKey(e: KeyPress, p: Platform): RecordResult {
  if (MODIFIER_CODES.has(e.code)) return { kind: 'modifiers', held: formatHeld(e, p) };
  const mods = heldMods(e);
  if (e.code === 'Escape' && mods.size === 0) return { kind: 'cancel' };
  const key = KEYS.get(registeredCode(e, p));
  if (!key) return { kind: 'invalid', reason: 'That key can’t be used in a shortcut.' };
  if (!isFunctionKey(e.code) && !mods.has('ctrl') && !mods.has('alt') && !mods.has('super')) {
    return { kind: 'invalid', reason: needModifierHint(p) };
  }
  const tokens: string[] = [];
  if (p === 'mac') {
    if (mods.has('super')) tokens.push('CommandOrControl');
    if (mods.has('ctrl')) tokens.push('Control');
  } else {
    if (mods.has('ctrl')) tokens.push('CommandOrControl');
    if (mods.has('super')) tokens.push('Super');
  }
  if (mods.has('alt')) tokens.push('Alt');
  if (mods.has('shift')) tokens.push('Shift');
  tokens.push(key.token);
  return { kind: 'accelerator', accelerator: tokens.join('+') };
}
