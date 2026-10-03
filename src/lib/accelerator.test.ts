import { describe, expect, it } from 'vitest';
import {
  type KeyPress,
  acceleratorParts,
  describeAccelerator,
  formatAccelerator,
  formatHeld,
  namesUsPosition,
  recordKey,
} from './accelerator';
import type { Platform } from './platform';

const DEFAULT = 'CommandOrControl+Alt+Shift+M';

// What the Keyboard Map API reports for a few keys of French AZERTY (base layer).
const AZERTY = new Map([
  ['KeyQ', 'a'],
  ['KeyA', 'q'],
  ['KeyM', ','],
  ['Semicolon', 'm'],
  ['Digit1', '&'],
  ['BracketLeft', 'Dead'],
]);

function press(code: string, mods: Partial<Omit<KeyPress, 'code'>> = {}): KeyPress {
  return { code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods };
}

function recorded(e: KeyPress, p: Platform): string {
  const result = recordKey(e, p);
  if (result.kind !== 'accelerator') throw new Error(`expected an accelerator, got ${result.kind}`);
  return result.accelerator;
}

// Key names global-hotkey 0.8 (tauri-plugin-global-shortcut 2.x) accepts, from its
// `parse_key`, upper-cased. Every accelerator the recorder makes must use one of these.
const PLUGIN_KEYS = new Set(
  [
    'BACKQUOTE BACKSLASH BRACKETLEFT BRACKETRIGHT PAUSE COMMA EQUAL MINUS PERIOD QUOTE',
    'SEMICOLON SLASH BACKSPACE CAPSLOCK ENTER SPACE TAB DELETE END HOME INSERT PAGEDOWN PAGEUP',
    'PRINTSCREEN SCROLLLOCK ARROWDOWN ARROWLEFT ARROWRIGHT ARROWUP NUMLOCK ESCAPE',
    'NUMPADADD NUMPADDECIMAL NUMPADDIVIDE NUMPADENTER NUMPADEQUAL NUMPADMULTIPLY NUMPADSUBTRACT',
  ]
    .join(' ')
    .split(' ')
    .concat(Array.from({ length: 26 }, (_, i) => String.fromCharCode(65 + i)))
    .concat(Array.from({ length: 10 }, (_, i) => `${i}`))
    .concat(Array.from({ length: 10 }, (_, i) => `NUMPAD${i}`))
    .concat(Array.from({ length: 24 }, (_, i) => `F${i + 1}`)),
);
const PLUGIN_MODIFIERS = new Set([
  'OPTION',
  'ALT',
  'CONTROL',
  'CTRL',
  'COMMAND',
  'CMD',
  'SUPER',
  'SHIFT',
  'COMMANDORCONTROL',
]);

/** True when global-hotkey's parser would accept `accelerator` (modifiers first, one key). */
function pluginAccepts(accelerator: string): boolean {
  const tokens = accelerator.split('+').map((t) => t.toUpperCase());
  const key = tokens.pop();
  return !!key && PLUGIN_KEYS.has(key) && tokens.every((t) => PLUGIN_MODIFIERS.has(t));
}

describe('formatAccelerator', () => {
  it('uses symbols in Apple order on macOS', () => {
    expect(formatAccelerator(DEFAULT, 'mac')).toBe('⌥⇧⌘M');
    expect(formatAccelerator('Control+Alt+Shift+Super+K', 'mac')).toBe('⌃⌥⇧⌘K');
    expect(acceleratorParts(DEFAULT, 'mac')).toEqual(['⌥', '⇧', '⌘', 'M']);
  });

  it('spells modifiers out and joins with + on Windows and Linux', () => {
    expect(formatAccelerator(DEFAULT, 'windows')).toBe('Ctrl+Alt+Shift+M');
    expect(formatAccelerator(DEFAULT, 'linux')).toBe('Ctrl+Alt+Shift+M');
    expect(formatAccelerator('Super+Shift+F5', 'windows')).toBe('Shift+Win+F5');
    expect(formatAccelerator('Super+Shift+F5', 'linux')).toBe('Shift+Super+F5');
  });

  it('maps CommandOrControl to ⌘ on macOS and Ctrl elsewhere', () => {
    expect(formatAccelerator('CmdOrCtrl+K', 'mac')).toBe('⌘K');
    expect(formatAccelerator('CmdOrCtrl+K', 'windows')).toBe('Ctrl+K');
  });

  it('accepts the aliases and any case the plugin accepts', () => {
    expect(formatAccelerator('option+shift+space', 'mac')).toBe('⌥⇧Space');
    expect(formatAccelerator('Ctrl+Esc', 'windows')).toBe('Ctrl+Esc');
    expect(formatAccelerator('Alt+Up', 'mac')).toBe('⌥↑');
    expect(formatAccelerator('Alt+KeyQ', 'linux')).toBe('Alt+Q');
    expect(formatAccelerator('Ctrl+Num5', 'windows')).toBe('Ctrl+Num5');
    expect(formatAccelerator('Cmd + Shift + /', 'mac')).toBe('⇧⌘/');
  });

  it('shows special keys with platform labels', () => {
    expect(formatAccelerator('Alt+Enter', 'mac')).toBe('⌥↩');
    expect(formatAccelerator('Alt+Enter', 'windows')).toBe('Alt+Enter');
    expect(formatAccelerator('Ctrl+ArrowLeft', 'windows')).toBe('Ctrl+Left');
    expect(formatAccelerator('Ctrl+Backquote', 'linux')).toBe('Ctrl+`');
  });

  it('shows an unknown key as written', () => {
    expect(formatAccelerator('Alt+MediaPlay', 'mac')).toBe('⌥MediaPlay');
  });
});

describe('describeAccelerator', () => {
  it('reads modifiers as words', () => {
    expect(describeAccelerator(DEFAULT, 'mac')).toBe('Option Shift Command M');
    expect(describeAccelerator(DEFAULT, 'windows')).toBe('Control Alt Shift M');
    expect(describeAccelerator('Super+Space', 'linux')).toBe('Super Space');
  });

  it('reads the layout’s character for a key position on macOS', () => {
    expect(describeAccelerator(DEFAULT, 'mac', AZERTY)).toBe('Option Shift Command ,');
  });
});

describe('key labels and keyboard layouts', () => {
  it('labels a macOS key position with the layout’s character when the webview knows it', () => {
    // global-hotkey registers "M" as the key right of N, which types "," on AZERTY.
    expect(formatAccelerator(DEFAULT, 'mac', AZERTY)).toBe('⌥⇧⌘,');
    expect(formatAccelerator('Alt+Semicolon', 'mac', AZERTY)).toBe('⌥M');
    expect(formatAccelerator('Alt+Q', 'mac', AZERTY)).toBe('⌥A');
    expect(formatAccelerator('Alt+1', 'mac', AZERTY)).toBe('⌥&');
  });

  it('keeps the US name for dead keys, unknown keys and keys the layout never changes', () => {
    expect(formatAccelerator('Alt+BracketLeft', 'mac', AZERTY)).toBe('⌥[');
    expect(formatAccelerator('Alt+K', 'mac', AZERTY)).toBe('⌥K');
    expect(formatAccelerator('Alt+Space', 'mac', AZERTY)).toBe('⌥Space');
    expect(formatAccelerator('Alt+F5', 'mac', new Map([['F5', 'x']]))).toBe('⌥F5');
  });

  it('ignores the layout map on Windows and Linux, where a token is the character itself', () => {
    expect(formatAccelerator(DEFAULT, 'windows', AZERTY)).toBe('Ctrl+Alt+Shift+M');
    expect(formatAccelerator(DEFAULT, 'linux', AZERTY)).toBe('Ctrl+Alt+Shift+M');
  });

  it('says when a macOS label names a US key position', () => {
    expect(namesUsPosition(DEFAULT, 'mac')).toBe(true);
    expect(namesUsPosition('Alt+Slash', 'mac', null)).toBe(true);
    expect(namesUsPosition(DEFAULT, 'mac', AZERTY)).toBe(false);
    expect(namesUsPosition('Alt+Space', 'mac')).toBe(false);
    expect(namesUsPosition('F13', 'mac')).toBe(false);
    expect(namesUsPosition(DEFAULT, 'windows')).toBe(false);
    expect(namesUsPosition(DEFAULT, 'linux')).toBe(false);
  });
});

describe('formatHeld', () => {
  it('shows the modifiers held so far', () => {
    expect(formatHeld(press('AltLeft', { altKey: true, shiftKey: true }), 'mac')).toBe('⌥⇧');
    expect(formatHeld(press('ControlLeft', { ctrlKey: true, altKey: true }), 'windows')).toBe(
      'Ctrl+Alt',
    );
    expect(formatHeld(press('ShiftLeft'), 'mac')).toBe('');
  });
});

describe('recordKey', () => {
  it('records the default shortcut on macOS from ⌥⇧⌘M', () => {
    const e = press('KeyM', { metaKey: true, altKey: true, shiftKey: true });
    expect(recorded(e, 'mac')).toBe(DEFAULT);
  });

  it('records the default shortcut on Windows and Linux from Ctrl+Alt+Shift+M', () => {
    const e = press('KeyM', { ctrlKey: true, altKey: true, shiftKey: true });
    expect(recorded(e, 'windows')).toBe(DEFAULT);
    expect(recorded(e, 'linux')).toBe(DEFAULT);
  });

  it('keeps Control on macOS and the Windows key elsewhere as their own modifiers', () => {
    expect(recorded(press('KeyK', { ctrlKey: true }), 'mac')).toBe('Control+K');
    expect(recorded(press('KeyK', { ctrlKey: true, metaKey: true }), 'mac')).toBe(
      'CommandOrControl+Control+K',
    );
    expect(recorded(press('KeyK', { metaKey: true }), 'windows')).toBe('Super+K');
  });

  it('uses the physical key on macOS, where the plugin registers key positions', () => {
    // ⌥M types "µ" on a US Mac layout; the code is still KeyM.
    expect(recorded(press('KeyM', { altKey: true }), 'mac')).toBe('Alt+M');
    expect(recorded(press('Digit2', { altKey: true, shiftKey: true }), 'mac')).toBe(
      'Alt+Shift+2',
    );
    // AZERTY's M key sits where US has ";": WebKit's keyCode follows the character, but the
    // position is what gets registered.
    expect(recorded(press('Semicolon', { keyCode: 77, metaKey: true }), 'mac')).toBe(
      'CommandOrControl+Semicolon',
    );
  });

  it('uses the virtual key on Windows, where the plugin registers virtual keys', () => {
    const azertyM = press('Semicolon', { keyCode: 77, ctrlKey: true, altKey: true, shiftKey: true });
    expect(recorded(azertyM, 'windows')).toBe(DEFAULT);
    // The key US calls M types "," on AZERTY, and has VK_OEM_COMMA.
    expect(recorded(press('KeyM', { keyCode: 188, ctrlKey: true }), 'windows')).toBe(
      'CommandOrControl+Comma',
    );
    // QWERTZ swaps Y and Z.
    expect(recorded(press('KeyY', { keyCode: 90, altKey: true }), 'windows')).toBe('Alt+Z');
    expect(recorded(press('Digit1', { keyCode: 49, altKey: true }), 'windows')).toBe('Alt+1');
  });

  it('uses the typed letter on Linux, where the plugin registers the key typing it', () => {
    expect(recorded(press('Semicolon', { keyCode: 77, altKey: true }), 'linux')).toBe('Alt+M');
    // There keyCode follows the shifted character ("?" for AZERTY's "," key), so digits and
    // punctuation keep their position.
    expect(recorded(press('KeyM', { keyCode: 191, altKey: true, shiftKey: true }), 'linux')).toBe(
      'Alt+Shift+M',
    );
  });

  it('falls back to the physical key when the virtual key is missing or not a character', () => {
    expect(recorded(press('KeyM', { altKey: true }), 'windows')).toBe('Alt+M');
    expect(recorded(press('KeyM', { keyCode: 229, altKey: true }), 'windows')).toBe('Alt+M');
    expect(recorded(press('KeyM', { keyCode: 0, altKey: true }), 'windows')).toBe('Alt+M');
    // Keys the layout never moves keep their code, whatever keyCode says (NumLock off, …).
    expect(recorded(press('Numpad5', { keyCode: 12, altKey: true }), 'windows')).toBe(
      'Alt+Numpad5',
    );
    expect(recorded(press('F5', { keyCode: 65, altKey: true }), 'windows')).toBe('Alt+F5');
  });

  it('reports held modifiers until a real key goes down', () => {
    expect(recordKey(press('MetaLeft', { metaKey: true }), 'mac')).toEqual({
      kind: 'modifiers',
      held: '⌘',
    });
    expect(recordKey(press('ShiftRight', { shiftKey: true, ctrlKey: true }), 'linux')).toEqual({
      kind: 'modifiers',
      held: 'Ctrl+Shift',
    });
    expect(recordKey(press('CapsLock'), 'mac').kind).toBe('modifiers');
  });

  it('cancels on a bare Escape, but records Escape with a modifier', () => {
    expect(recordKey(press('Escape'), 'mac')).toEqual({ kind: 'cancel' });
    expect(recorded(press('Escape', { altKey: true }), 'mac')).toBe('Alt+Escape');
  });

  it('needs a modifier other than Shift', () => {
    const bare = recordKey(press('KeyM'), 'mac');
    const shifted = recordKey(press('KeyM', { shiftKey: true }), 'windows');
    expect(bare.kind).toBe('invalid');
    expect(shifted.kind).toBe('invalid');
    if (bare.kind === 'invalid') expect(bare.reason).toContain('⌘');
    if (shifted.kind === 'invalid') expect(shifted.reason).toContain('Ctrl');
    if (bare.kind === 'invalid') expect(bare.reason).toContain('F1–F24');
  });

  it('accepts F1–F24 alone or with Shift only, like the app', () => {
    for (const p of ['mac', 'windows', 'linux'] as const) {
      expect(recorded(press('F13'), p)).toBe('F13');
      expect(recorded(press('F1'), p)).toBe('F1');
      expect(recorded(press('F24', { shiftKey: true }), p)).toBe('Shift+F24');
      expect(pluginAccepts(recorded(press('F13'), p))).toBe(true);
    }
    expect(recorded(press('F5', { altKey: true }), 'mac')).toBe('Alt+F5');
    expect(recordKey(press('Space', { shiftKey: true }), 'mac').kind).toBe('invalid');
  });

  it('rejects keys the plugin cannot register', () => {
    const result = recordKey(press('IntlBackslash', { ctrlKey: true }), 'windows');
    expect(result.kind).toBe('invalid');
    expect(recordKey(press('', { ctrlKey: true }), 'windows').kind).toBe('invalid');
  });

  it('only makes accelerators the global-shortcut plugin accepts, and formats them back', () => {
    const codes = [
      ...Array.from({ length: 26 }, (_, i) => `Key${String.fromCharCode(65 + i)}`),
      ...Array.from({ length: 10 }, (_, i) => `Digit${i}`),
      ...Array.from({ length: 10 }, (_, i) => `Numpad${i}`),
      ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
      'Space',
      'Enter',
      'Tab',
      'Backspace',
      'Delete',
      'Escape',
      'ArrowUp',
      'ArrowDown',
      'ArrowLeft',
      'ArrowRight',
      'Home',
      'End',
      'PageUp',
      'PageDown',
      'Insert',
      'PrintScreen',
      'ScrollLock',
      'Pause',
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
      'NumpadAdd',
      'NumpadSubtract',
      'NumpadMultiply',
      'NumpadDivide',
      'NumpadDecimal',
      'NumpadEnter',
      'NumpadEqual',
    ];
    for (const p of ['mac', 'windows', 'linux'] as const) {
      for (const code of codes) {
        const accelerator = recorded(press(code, { altKey: true, shiftKey: true }), p);
        expect(pluginAccepts(accelerator), `${p} ${code} → ${accelerator}`).toBe(true);
        const parts = acceleratorParts(accelerator, p);
        expect(parts).toHaveLength(3);
        expect(parts.slice(0, 2)).toEqual(p === 'mac' ? ['⌥', '⇧'] : ['Alt', 'Shift']);
      }
    }
  });

  it('keeps nothing but the accelerator string', () => {
    const result = recordKey(press('KeyA', { metaKey: true }), 'mac');
    expect(result).toEqual({ kind: 'accelerator', accelerator: 'CommandOrControl+A' });
  });
});
