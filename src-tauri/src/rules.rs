//! Per-app rules and the input gate (`docs/ui-contract.md` § Per-app rules, § The gate): pure
//! functions, unit-tested. The platform side (which app is in front) lives in `apps.rs`.
//!
//! The key hook checks one `AtomicBool`; [`gate_open`] is its value, recomputed off the hook
//! thread on every state change (`Shared::update`).

use crate::state::{AppRule, AppRuleEntry, AppRuleMode, MAX_RULE_APPS, OWN_APP_ID};
use std::time::{Duration, Instant};

/// The longest bundle identifier `add_rule_app` accepts.
pub const MAX_ID_LEN: usize = 255;
/// A rule block keeps the output open this long before closing it, so hopping through a blocked
/// app with ⌘Tab does not reopen the device.
pub const CLOSE_AFTER: Duration = Duration::from_secs(5);

/// `add_rule_app`: the id is empty, too long, or contains whitespace or control characters.
pub const NOT_AN_APP: &str = "That is not an app TakTak can recognize.";
/// `add_rule_app`: TakTak's own id.
pub const OWN_APP: &str =
    "TakTak itself can’t be listed: its windows always follow the app you were in.";
/// `add_rule_app`: the list is full.
pub const TOO_MANY: &str = "You can list up to 200 apps.";

/// Whether key events make sounds: sounds on, not muted by hand, not auto-muted, and not
/// silenced by a per-app rule. The hook's single atomic holds this.
pub fn gate_open(enabled: bool, muted: bool, auto_muted: bool, rule_blocked: bool) -> bool {
    enabled && !muted && !auto_muted && !rule_blocked
}

/// Whether `rule` silences the app in front (`frontmost`: its bundle id; `None` = unknown).
/// "only" with an unknown app is silent, and an empty "only" list is silent everywhere.
pub fn blocks(rule: &AppRule, frontmost: Option<&str>) -> bool {
    let listed = frontmost.is_some_and(|id| rule.apps.iter().any(|a| a.id == id));
    match rule.mode {
        AppRuleMode::Everywhere => false,
        AppRuleMode::Only => !listed,
        AppRuleMode::Never => listed,
    }
}

/// Whether `id` (already trimmed) looks like a bundle identifier TakTak can list.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_ID_LEN
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// `add_rule_app`: appends `{ id, name }` (both trimmed; an empty name becomes the id). An id
/// that is already listed changes nothing. Returns whether the list changed, or the message for
/// the user.
pub fn add(rule: &mut AppRule, id: &str, name: &str) -> Result<bool, &'static str> {
    let id = id.trim();
    if !valid_id(id) {
        return Err(NOT_AN_APP);
    }
    if id == OWN_APP_ID {
        return Err(OWN_APP);
    }
    if rule.apps.iter().any(|a| a.id == id) {
        return Ok(false);
    }
    if rule.apps.len() >= MAX_RULE_APPS {
        return Err(TOO_MANY);
    }
    let name = name.trim();
    let name = if name.is_empty() { id } else { name };
    rule.apps.push(AppRuleEntry { id: id.to_owned(), name: name.to_owned() });
    Ok(true)
}

/// `remove_rule_app`: removes `id`'s entry; returns whether there was one.
pub fn remove(rule: &mut AppRule, id: &str) -> bool {
    let before = rule.apps.len();
    rule.apps.retain(|a| a.id != id);
    rule.apps.len() != before
}

/// `rule` as it may be stored: ids trimmed and non-empty, later duplicates dropped, empty
/// names replaced by the id, at most [`MAX_RULE_APPS`] entries (contract § Settings migration).
pub fn sanitize(rule: AppRule) -> AppRule {
    let mut apps: Vec<AppRuleEntry> = Vec::with_capacity(rule.apps.len().min(MAX_RULE_APPS));
    for entry in rule.apps {
        if apps.len() == MAX_RULE_APPS {
            break;
        }
        let id = entry.id.trim();
        if id.is_empty() || apps.iter().any(|a| a.id == id) {
            continue;
        }
        let name = entry.name.trim();
        let name = if name.is_empty() { id } else { name };
        apps.push(AppRuleEntry { id: id.to_owned(), name: name.to_owned() });
    }
    AppRule { mode: rule.mode, apps }
}

/// How long a rule block has lasted, for closing the output only once it has lasted
/// [`CLOSE_AFTER`] (`docs/ui-contract.md` § Behaviour rules, Power). Kept by the control thread,
/// which hears about every change of the block (the state change wakes it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuleBlock {
    since: Option<Instant>,
}

impl RuleBlock {
    /// Records whether the rules block the app in front now.
    pub fn observe(&mut self, blocked: bool, now: Instant) {
        if !blocked {
            self.since = None;
        } else if self.since.is_none() {
            self.since = Some(now);
        }
    }

    /// The block has lasted [`CLOSE_AFTER`]: the output and the listener may close.
    pub fn settled(&self, now: Instant) -> bool {
        self.settles_at().is_some_and(|at| now >= at)
    }

    /// When the current block will have lasted [`CLOSE_AFTER`] (one wake-up, not a poll).
    pub fn settles_at(&self) -> Option<Instant> {
        self.since.map(|since| since + CLOSE_AFTER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str) -> AppRuleEntry {
        AppRuleEntry { id: id.into(), name: name.into() }
    }

    fn rule(mode: AppRuleMode, ids: &[&str]) -> AppRule {
        AppRule { mode, apps: ids.iter().map(|id| entry(id, id)).collect() }
    }

    #[test]
    fn modes_follow_the_contract() {
        let slack = Some("com.tinyspeck.slackmacgap");
        let safari = Some("com.apple.Safari");
        let listed = ["com.tinyspeck.slackmacgap", "us.zoom.xos"];

        let everywhere = rule(AppRuleMode::Everywhere, &listed);
        assert!(!blocks(&everywhere, slack), "the list is kept but ignored");
        assert!(!blocks(&everywhere, None));

        let only = rule(AppRuleMode::Only, &listed);
        assert!(!blocks(&only, slack));
        assert!(blocks(&only, safari));
        assert!(blocks(&only, None), "an unknown app is not listed");
        assert!(blocks(&rule(AppRuleMode::Only, &[]), safari), "an empty list: silent everywhere");

        let never = rule(AppRuleMode::Never, &listed);
        assert!(blocks(&never, slack));
        assert!(!blocks(&never, safari));
        assert!(!blocks(&never, None));
        assert!(!blocks(&rule(AppRuleMode::Never, &[]), slack));
    }

    #[test]
    fn the_gate_needs_every_term() {
        assert!(gate_open(true, false, false, false));
        for (enabled, muted, auto, blocked) in [
            (false, false, false, false),
            (true, true, false, false),
            (true, false, true, false),
            (true, false, false, true),
            (false, true, true, true),
        ] {
            assert!(
                !gate_open(enabled, muted, auto, blocked),
                "{enabled} {muted} {auto} {blocked}"
            );
        }
    }

    #[test]
    fn adding_validates_trims_and_ignores_duplicates() {
        let mut r = AppRule::default();
        assert_eq!(add(&mut r, " com.apple.Safari ", " Safari "), Ok(true));
        assert_eq!(r.apps, [entry("com.apple.Safari", "Safari")]);
        // Already listed: nothing changes, the entry keeps its fields, no error.
        assert_eq!(add(&mut r, "com.apple.Safari", "Other"), Ok(false));
        assert_eq!(r.apps, [entry("com.apple.Safari", "Safari")]);
        // An empty name becomes the id.
        assert_eq!(add(&mut r, "us.zoom.xos", "  "), Ok(true));
        assert_eq!(r.apps[1], entry("us.zoom.xos", "us.zoom.xos"));

        for bad in ["", "   ", "com.apple Safari", "com.apple.\tSafari", "a\u{7}b", "a\u{85}b"] {
            assert_eq!(add(&mut r, bad, "x"), Err(NOT_AN_APP), "{bad:?}");
        }
        assert_eq!(add(&mut r, &"a".repeat(MAX_ID_LEN + 1), "x"), Err(NOT_AN_APP));
        assert_eq!(add(&mut r, &"é".repeat(MAX_ID_LEN), "x"), Ok(true), "characters, not bytes");
        assert_eq!(add(&mut r, OWN_APP_ID, "TakTak"), Err(OWN_APP));
        assert_eq!(add(&mut r, " tech.taktak.app ", "TakTak"), Err(OWN_APP));
        assert_eq!(r.apps.len(), 3);
    }

    #[test]
    fn the_list_holds_two_hundred_apps() {
        let mut r = AppRule::default();
        for i in 0..MAX_RULE_APPS {
            assert_eq!(add(&mut r, &format!("com.example.app{i}"), "App"), Ok(true));
        }
        assert_eq!(add(&mut r, "com.example.one-more", "App"), Err(TOO_MANY));
        // A listed id is still "no change", not an error, when the list is full.
        assert_eq!(add(&mut r, "com.example.app7", "App"), Ok(false));
        assert!(remove(&mut r, "com.example.app7"));
        assert_eq!(add(&mut r, "com.example.one-more", "App"), Ok(true));
    }

    #[test]
    fn removing_an_unlisted_id_changes_nothing() {
        let mut r = rule(AppRuleMode::Never, &["a.b", "c.d"]);
        assert!(!remove(&mut r, "x.y"));
        assert!(remove(&mut r, "a.b"));
        assert_eq!(r.apps, [entry("c.d", "c.d")]);
        assert_eq!(r.mode, AppRuleMode::Never, "the mode stays");
    }

    #[test]
    fn sanitizing_trims_dedupes_names_and_caps() {
        let mut apps = vec![
            entry(" com.apple.Safari ", " Safari "),
            entry("", "Nameless"),
            entry("   ", "Blank"),
            entry("com.apple.Safari", "Safari again"),
            entry("us.zoom.xos", ""),
        ];
        apps.extend((0..MAX_RULE_APPS).map(|i| entry(&format!("com.example.app{i}"), "App")));
        let r = sanitize(AppRule { mode: AppRuleMode::Only, apps });
        assert_eq!(r.mode, AppRuleMode::Only);
        assert_eq!(r.apps.len(), MAX_RULE_APPS);
        assert_eq!(r.apps[0], entry("com.apple.Safari", "Safari"));
        assert_eq!(r.apps[1], entry("us.zoom.xos", "us.zoom.xos"));
        assert_eq!(r.apps[2].id, "com.example.app0");
        assert_eq!(r.apps.last().unwrap().id, format!("com.example.app{}", MAX_RULE_APPS - 3));
        // Already clean: unchanged.
        let clean = rule(AppRuleMode::Never, &["a.b", "c.d"]);
        assert_eq!(sanitize(clean.clone()), clean);
    }

    #[test]
    fn a_rule_block_closes_the_output_only_after_five_seconds() {
        let t0 = Instant::now();
        let mut block = RuleBlock::default();
        block.observe(false, t0);
        assert_eq!(block.settles_at(), None);
        assert!(!block.settled(t0));

        block.observe(true, t0);
        assert_eq!(block.settles_at(), Some(t0 + CLOSE_AFTER));
        // Seeing the same block again does not restart the clock.
        block.observe(true, t0 + Duration::from_secs(3));
        assert!(!block.settled(t0 + Duration::from_secs(4)));
        assert!(block.settled(t0 + CLOSE_AFTER));

        // ⌘Tab through a blocked app: the block ends before it settles.
        block.observe(false, t0 + Duration::from_secs(6));
        assert!(!block.settled(t0 + Duration::from_secs(6)));
        block.observe(true, t0 + Duration::from_secs(7));
        assert!(!block.settled(t0 + Duration::from_secs(8)));
        assert!(block.settled(t0 + Duration::from_secs(12)));
    }
}
