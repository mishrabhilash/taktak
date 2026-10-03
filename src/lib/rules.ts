// Per-app rules (docs/ui-contract.md § Per-app rules): how the list is edited, what each mode
// means, and what Settings says about the app in front. The app decides for real (`ruleBlocked`);
// these mirror its rules so an edit shows its effect at once (optimistic.ts) and the mock agrees.

import type { AppInfo, AppRef, AppRule, AppRuleMode, AppState } from './types';

/** TakTak's own bundle id: never listed (its windows follow the app the user came from). */
export const OWN_APP_ID = 'tech.taktak.app';
/** The most apps the list holds. */
export const MAX_RULE_APPS = 200;

export interface RuleModeInfo {
  mode: AppRuleMode;
  label: string;
  /** One line under the label. */
  hint: string;
}

/** The three modes, in the order Settings offers them. */
export const RULE_MODES: readonly RuleModeInfo[] = [
  {
    mode: 'everywhere',
    label: 'Everywhere',
    hint: 'TakTak plays in every app. The list below is kept but not used.',
  },
  {
    mode: 'only',
    label: 'Only in these apps',
    hint: 'TakTak plays only while one of the apps below is in front.',
  },
  {
    mode: 'never',
    label: 'Never in these apps',
    hint: 'TakTak plays everywhere except while one of the apps below is in front.',
  },
];

/** Whether `id` is in the list. */
export function isListed(rule: AppRule, id: string): boolean {
  return rule.apps.some((a) => a.id === id);
}

/** Whether `rule` silences `frontmost` (the app's `ruleBlocked`). */
export function ruleBlocks(rule: AppRule, frontmost: AppRef | null, supported: boolean): boolean {
  if (!supported) return false;
  const listed = frontmost !== null && isListed(rule, frontmost.id);
  switch (rule.mode) {
    case 'everywhere':
      return false;
    case 'only':
      // An unknown app is not listed, so it is silent; an empty list is silent everywhere.
      return !listed;
    case 'never':
      return listed;
  }
}

/**
 * Why `add_rule_app` would reject `id` (its exact message), or null when it would accept it.
 * An id that is already listed is accepted: it changes nothing.
 */
export function ruleAppProblem(rule: AppRule, id: string): string | null {
  const trimmed = id.trim();
  // Counted in characters (code points), like the app.
  if (!trimmed || [...trimmed].length > 255 || /[\s\p{Cc}]/u.test(trimmed)) {
    return 'That is not an app TakTak can recognize.';
  }
  if (trimmed === OWN_APP_ID) {
    return 'TakTak itself can’t be listed: its windows always follow the app you were in.';
  }
  if (!isListed(rule, trimmed) && rule.apps.length >= MAX_RULE_APPS) {
    return `You can list up to ${MAX_RULE_APPS} apps.`;
  }
  return null;
}

/**
 * `rule` with `app` appended, as `add_rule_app` stores it: trimmed, an empty name becomes the
 * id. An app that is already listed, or that the app would reject, leaves `rule` as it is.
 */
export function withApp(rule: AppRule, app: AppRef): AppRule {
  if (ruleAppProblem(rule, app.id) !== null) return rule;
  const id = app.id.trim();
  if (isListed(rule, id)) return rule;
  return { ...rule, apps: [...rule.apps, { id, name: app.name.trim() || id }] };
}

/** `rule` without `id` (unchanged when it is not listed). */
export function withoutApp(rule: AppRule, id: string): AppRule {
  if (!isListed(rule, id)) return rule;
  return { ...rule, apps: rule.apps.filter((a) => a.id !== id) };
}

/** `rule` with another mode; the list stays. */
export function withMode(rule: AppRule, mode: AppRuleMode): AppRule {
  return rule.mode === mode ? rule : { ...rule, mode };
}

/** Lower case without accents, for matching "safari" to "Safari" and "e" to "é". */
function fold(text: string): string {
  return text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
}

/**
 * The apps whose name or bundle id contains every word of `query` (case and accents ignored),
 * in their original order. An empty query matches all.
 */
export function searchApps(apps: readonly AppInfo[], query: string): AppInfo[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...apps];
  return apps.filter((app) => {
    const haystack = `${fold(app.name)} ${fold(app.id)}`;
    return words.every((w) => haystack.includes(w));
  });
}

/** What Settings says about the app in front (it is the one the user came from). */
export interface FrontmostSummary {
  /** A full sentence, e.g. "TakTak is silent in Terminal because of your rules." */
  text: string;
  /** The rules keep TakTak silent there (mute, permission and audio aside). */
  silent: boolean;
  /** The app is known and can be added to the list. */
  canAdd: boolean;
}

/** null where rules are unsupported. */
export function frontmostSummary(s: AppState): FrontmostSummary | null {
  if (!s.rulesSupported) return null;
  const rule = s.settings.appRule;
  const app = s.frontmostApp;
  const silent = s.ruleBlocked;
  const listed = app !== null && isListed(rule, app.id);
  const canAdd = app !== null && !listed && ruleAppProblem(rule, app.id) === null;
  if (app === null) {
    return {
      text: silent
        ? 'TakTak can’t tell which app is in front, so your rules keep it silent.'
        : 'TakTak can’t tell which app is in front right now.',
      silent,
      canAdd,
    };
  }
  let text: string;
  if (silent) text = `TakTak is silent in ${app.name} because of your rules.`;
  else if (rule.mode === 'only') text = `TakTak plays in ${app.name}: it’s on your list.`;
  else if (rule.mode === 'never') text = `TakTak plays in ${app.name}: it isn’t on your list.`;
  else text = `TakTak plays in ${app.name}.`;
  return { text, silent, canAdd };
}
