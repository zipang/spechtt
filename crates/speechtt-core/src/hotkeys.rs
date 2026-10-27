//! Canonical keyboard shortcuts and the OS hotkey listener seam.
//!
//! Shortcuts are parsed from human notation (`Ctrl+Shift+R`,
//! `ctrl + alt + space`, `Super+/`, `F9`) into a [`Shortcut`] whose `Display`
//! is the canonical `Ctrl+Alt+Shift+Super+Key` order. Parsing rejects empty
//! input, unknown modifiers, duplicate modifiers, missing keys and keys
//! outside the supported set.
//!
//! # Default bindings
//!
//! | Action | Default |
//! |---|---|
//! | `StartRecording` | `Ctrl+Shift+R` |
//! | `PauseRecording` | `Ctrl+Shift+P` |
//! | `ResumeRecording` | `Ctrl+Shift+O` |
//! | `StopRecording` | `Ctrl+Shift+S` |
//! | `NewTake` | `Ctrl+Shift+N` |
//! | `ToggleHelp` | `Ctrl+Shift+H` |
//! | `ToggleSettings` | `Ctrl+Shift+,` |
//!
//! Pause and Resume are deliberately distinct actions: Pause freezes the take,
//! Resume continues it, and each keeps its own global shortcut. `Resume` uses
//! `Ctrl+Shift+O` (not `Ctrl+Shift+P`) so the two never collide.
//!
//! [`ShortcutListener`] is the OS seam. [`GlobalHotkeyListener`] implements it
//! with the `global-hotkey` crate and must be constructed on the main thread;
//! tests only assert that it implements the trait and exercise the pure
//! shortcut -> hotkey conversion instead of opening a display server.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use global_hotkey::GlobalHotKeyEvent;
use global_hotkey::GlobalHotKeyEventReceiver;
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// A user-visible session command that can be bound to a global shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionAction {
    StartRecording,
    PauseRecording,
    ResumeRecording,
    StopRecording,
    NewTake,
    ToggleHelp,
    ToggleSettings,
}

impl SessionAction {
    /// Every action, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::StartRecording,
        Self::PauseRecording,
        Self::ResumeRecording,
        Self::StopRecording,
        Self::NewTake,
        Self::ToggleHelp,
        Self::ToggleSettings,
    ];

    /// The stable name used in [`Settings::shortcuts`] and serde.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StartRecording => "startRecording",
            Self::PauseRecording => "pauseRecording",
            Self::ResumeRecording => "resumeRecording",
            Self::StopRecording => "stopRecording",
            Self::NewTake => "newTake",
            Self::ToggleHelp => "toggleHelp",
            Self::ToggleSettings => "toggleSettings",
        }
    }
}

impl fmt::Display for SessionAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SessionAction {
    type Err = ShortcutError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str().eq_ignore_ascii_case(value))
            .ok_or_else(|| ShortcutError::UnknownAction(value.to_owned()))
    }
}

/// A parsed keyboard shortcut with canonical, upper-case key naming.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// Canonical key: a single upper-case character, `F1`..`F12`, `Space`, ...
    pub key: String,
}

impl Shortcut {
    /// Parses human shortcut notation.
    ///
    /// # Errors
    ///
    /// Returns [`ShortcutError`] for empty input, unknown or duplicate
    /// modifiers, a missing key, or an unsupported key.
    pub fn parse(input: &str) -> Result<Self, ShortcutError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(ShortcutError::Empty);
        }

        let tokens: Vec<&str> = trimmed.split('+').map(str::trim).collect();
        if tokens.iter().any(|token| token.is_empty()) {
            return Err(ShortcutError::MissingKey(trimmed.to_owned()));
        }

        let Some((key_token, modifier_tokens)) = tokens.split_last() else {
            return Err(ShortcutError::Empty);
        };
        let key_token = *key_token;

        // A trailing modifier means the user stopped before naming a key.
        if modifier_kind(key_token).is_some() {
            return Err(ShortcutError::MissingKey(trimmed.to_owned()));
        }

        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut meta = false;

        for token in modifier_tokens {
            match modifier_kind(token) {
                Some(Modifier::Ctrl) => set_modifier(&mut ctrl, token)?,
                Some(Modifier::Alt) => set_modifier(&mut alt, token)?,
                Some(Modifier::Shift) => set_modifier(&mut shift, token)?,
                Some(Modifier::Meta) => set_modifier(&mut meta, token)?,
                None => return Err(ShortcutError::UnknownModifier((*token).to_owned())),
            }
        }

        let key = canonical_key(key_token)
            .ok_or_else(|| ShortcutError::UnknownKey(key_token.to_owned()))?;

        Ok(Self {
            ctrl,
            alt,
            shift,
            meta,
            key,
        })
    }

    /// Whether both shortcuts describe the same binding (key comparison is
    /// ASCII case-insensitive; modifiers must match exactly).
    #[must_use]
    pub fn same_binding(&self, other: &Self) -> bool {
        self.ctrl == other.ctrl
            && self.alt == other.alt
            && self.shift == other.shift
            && self.meta == other.meta
            && self.key.eq_ignore_ascii_case(&other.key)
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::with_capacity(5);

        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.meta {
            parts.push("Super");
        }
        parts.push(self.key.as_str());

        formatter.write_str(&parts.join("+"))
    }
}

/// Modifier names accepted by [`Shortcut::parse`].
#[derive(Debug, Clone, Copy)]
enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

fn modifier_kind(token: &str) -> Option<Modifier> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(Modifier::Ctrl),
        "alt" | "option" | "opt" => Some(Modifier::Alt),
        "shift" => Some(Modifier::Shift),
        "super" | "meta" | "cmd" | "command" | "win" | "windows" => Some(Modifier::Meta),
        _ => None,
    }
}

fn set_modifier(flag: &mut bool, token: &str) -> Result<(), ShortcutError> {
    if *flag {
        return Err(ShortcutError::DuplicateModifier(token.to_owned()));
    }

    *flag = true;
    Ok(())
}

/// Canonical key naming: upper-case single characters, `F1`..`F12`, and named
/// navigation/editing keys.
fn canonical_key(token: &str) -> Option<String> {
    let named = match token.to_ascii_lowercase().as_str() {
        "space" => "Space",
        "tab" => "Tab",
        "enter" | "return" => "Enter",
        "escape" | "esc" => "Escape",
        "backspace" => "Backspace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdn" => "PageDown",
        "up" | "arrowup" => "Up",
        "down" | "arrowdown" => "Down",
        "left" | "arrowleft" => "Left",
        "right" | "arrowright" => "Right",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }

    if let Some(rest) = token.to_ascii_lowercase().strip_prefix('f')
        && let Ok(number) = rest.parse::<u8>()
        && (1..=12).contains(&number)
    {
        return Some(format!("F{number}"));
    }

    let mut characters = token.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }

    match character {
        'a'..='z' => Some(character.to_ascii_uppercase().to_string()),
        'A'..='Z' | '0'..='9' => Some(character.to_string()),
        '/' | '\\' | ',' | '.' | ';' | '\'' | '[' | ']' | '-' | '=' | '`' => {
            Some(character.to_string())
        }
        _ => None,
    }
}

/// Why a shortcut or binding operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShortcutError {
    #[error("shortcut is empty")]
    Empty,
    #[error("unknown modifier {0:?}")]
    UnknownModifier(String),
    #[error("duplicate modifier {0:?}")]
    DuplicateModifier(String),
    #[error("shortcut {0:?} has no key")]
    MissingKey(String),
    #[error("unsupported key {0:?}")]
    UnknownKey(String),
    #[error("unknown session action {0:?}")]
    UnknownAction(String),
    #[error("{action} is already bound to {shortcut}")]
    Conflict {
        action: SessionAction,
        shortcut: Shortcut,
    },
}

/// Why the OS hotkey backend failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HotkeyError {
    #[error("could not initialise the global hotkey backend: {0}")]
    Backend(String),
    #[error("the os hotkey backend does not support key {key:?}")]
    UnsupportedKey { key: String },
    #[error("could not register {shortcut}: {reason}")]
    Register { shortcut: Shortcut, reason: String },
    #[error(transparent)]
    Shortcut(#[from] ShortcutError),
}

/// The OS seam: register a set of action/shortcut pairs or drop them all.
pub trait ShortcutListener: Send {
    /// Replaces the active registrations with `shortcuts`.
    ///
    /// # Errors
    ///
    /// Returns [`HotkeyError`] when the OS rejects a shortcut. Previously
    /// active registrations are dropped.
    fn register(&mut self, shortcuts: &[(SessionAction, Shortcut)]) -> Result<(), HotkeyError>;

    /// Removes every registration owned by this listener.
    fn unregister_all(&mut self);
}

/// The default action -> shortcut table (see the module docs).
#[must_use]
pub fn default_bindings() -> BTreeMap<SessionAction, Shortcut> {
    BTreeMap::from([
        (SessionAction::StartRecording, ctrl_shift("R")),
        (SessionAction::PauseRecording, ctrl_shift("P")),
        (SessionAction::ResumeRecording, ctrl_shift("O")),
        (SessionAction::StopRecording, ctrl_shift("S")),
        (SessionAction::NewTake, ctrl_shift("N")),
        (SessionAction::ToggleHelp, ctrl_shift("H")),
        (SessionAction::ToggleSettings, ctrl_shift(",")),
    ])
}

/// Builds a canonical `Ctrl+Shift+<key>` shortcut from a static key name.
fn ctrl_shift(key: &str) -> Shortcut {
    Shortcut {
        ctrl: true,
        alt: false,
        shift: true,
        meta: false,
        key: key.to_owned(),
    }
}

/// Finds the action bound to `shortcut`, comparing modifiers exactly and keys
/// case-insensitively.
#[must_use]
pub fn resolve(
    bindings: &BTreeMap<SessionAction, Shortcut>,
    shortcut: &Shortcut,
) -> Option<SessionAction> {
    bindings
        .iter()
        .find(|(_, bound)| bound.same_binding(shortcut))
        .map(|(action, _)| *action)
}

/// Every pair of actions bound to the same shortcut.
#[must_use]
pub fn conflicts(
    bindings: &BTreeMap<SessionAction, Shortcut>,
) -> Vec<(SessionAction, SessionAction, Shortcut)> {
    let entries: Vec<(&SessionAction, &Shortcut)> = bindings.iter().collect();
    let mut conflicts = Vec::new();

    for (index, (action, shortcut)) in entries.iter().enumerate() {
        for (other_action, other_shortcut) in entries.iter().skip(index + 1) {
            if shortcut.same_binding(other_shortcut) {
                conflicts.push((**action, **other_action, (*shortcut).clone()));
            }
        }
    }

    conflicts
}

/// Binds `action` to `shortcut`, replacing that action's own binding.
///
/// # Errors
///
/// Returns [`ShortcutError::Conflict`] when a different action already owns
/// the same shortcut; the map is left unchanged.
pub fn bind(
    bindings: &mut BTreeMap<SessionAction, Shortcut>,
    action: SessionAction,
    shortcut: Shortcut,
) -> Result<(), ShortcutError> {
    let duplicate = bindings
        .iter()
        .find(|(bound_action, bound)| **bound_action != action && bound.same_binding(&shortcut))
        .map(|(bound_action, bound)| (*bound_action, bound.clone()));

    if let Some((bound_action, bound)) = duplicate {
        return Err(ShortcutError::Conflict {
            action: bound_action,
            shortcut: bound,
        });
    }

    bindings.insert(action, shortcut);
    Ok(())
}

/// Parses [`Settings::shortcuts`] over the default bindings.
///
/// An empty map yields [`default_bindings`]. Persisted entries override
/// individual defaults, so malformed strings or conflicting overrides surface
/// as [`ShortcutError`] instead of silently disabling hotkeys.
///
/// # Errors
///
/// Returns [`ShortcutError::UnknownAction`] or [`ShortcutError::UnknownKey`]
/// and friends for unparsable entries, and [`ShortcutError::Conflict`] when
/// an override collides with another action.
pub fn bindings_from_settings(
    settings: &Settings,
) -> Result<BTreeMap<SessionAction, Shortcut>, ShortcutError> {
    let mut bindings = default_bindings();

    for (name, shortcut) in &settings.shortcuts {
        let action = SessionAction::from_str(name)?;
        let shortcut = Shortcut::parse(shortcut)?;
        bind(&mut bindings, action, shortcut)?;
    }

    Ok(bindings)
}

/// Owns a [`ShortcutListener`] and unregisters everything when dropped.
///
/// The UI keeps one guard for the process lifetime; dropping it on shutdown
/// releases the OS grabs. Explicit `unregister_all` calls are idempotent.
pub struct ListenerGuard<L: ShortcutListener> {
    listener: L,
}

impl<L: ShortcutListener> ListenerGuard<L> {
    pub fn new(listener: L) -> Self {
        Self { listener }
    }

    /// Registers `shortcuts` on the owned listener.
    ///
    /// # Errors
    ///
    /// Forwards [`ShortcutListener::register`] failures.
    pub fn register(&mut self, shortcuts: &[(SessionAction, Shortcut)]) -> Result<(), HotkeyError> {
        self.listener.register(shortcuts)
    }

    /// Explicitly releases all registrations.
    pub fn unregister_all(&mut self) {
        self.listener.unregister_all();
    }

    /// Access to the concrete listener (for example to read its event
    /// receiver).
    pub fn listener(&self) -> &L {
        &self.listener
    }

    /// Mutable access to the concrete listener.
    pub fn listener_mut(&mut self) -> &mut L {
        &mut self.listener
    }
}

impl<L: ShortcutListener> Drop for ListenerGuard<L> {
    fn drop(&mut self) {
        self.listener.unregister_all();
    }
}

/// A real OS listener backed by `global-hotkey` (X11 on Linux).
///
/// Construct this only on the main thread; the backend needs the platform
/// event loop to be running. Tests never instantiate it.
pub struct GlobalHotkeyListener {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
    actions: BTreeMap<u32, SessionAction>,
}

impl GlobalHotkeyListener {
    /// Creates the platform manager.
    ///
    /// # Errors
    ///
    /// Returns [`HotkeyError::Backend`] when the OS cannot provide a hotkey
    /// manager (for example no X11 display).
    pub fn new() -> Result<Self, HotkeyError> {
        let manager =
            GlobalHotKeyManager::new().map_err(|error| HotkeyError::Backend(error.to_string()))?;
        Ok(Self {
            manager,
            registered: Vec::new(),
            actions: BTreeMap::new(),
        })
    }

    /// The process-wide event channel; pair `event.id()` with
    /// [`Self::action_for_id`].
    #[must_use]
    pub fn event_receiver(&self) -> GlobalHotKeyEventReceiver {
        GlobalHotKeyEvent::receiver().clone()
    }

    /// The action that owns a hotkey id, if this listener registered it.
    #[must_use]
    pub fn action_for_id(&self, id: u32) -> Option<SessionAction> {
        self.actions.get(&id).copied()
    }

    fn rollback(&mut self, hotkeys: &[HotKey]) {
        for hotkey in hotkeys {
            let _ = self.manager.unregister(*hotkey);
        }
        self.actions.clear();
    }
}

impl ShortcutListener for GlobalHotkeyListener {
    fn register(&mut self, shortcuts: &[(SessionAction, Shortcut)]) -> Result<(), HotkeyError> {
        self.unregister_all();

        let mut registered = Vec::with_capacity(shortcuts.len());

        for (action, shortcut) in shortcuts {
            let hotkey = match to_hotkey(shortcut) {
                Ok(hotkey) => hotkey,
                Err(error) => {
                    self.rollback(&registered);
                    return Err(error);
                }
            };

            if let Err(error) = self.manager.register(hotkey) {
                self.rollback(&registered);
                return Err(HotkeyError::Register {
                    shortcut: shortcut.clone(),
                    reason: error.to_string(),
                });
            }

            self.actions.insert(hotkey.id(), *action);
            registered.push(hotkey);
        }

        self.registered = registered;
        Ok(())
    }

    fn unregister_all(&mut self) {
        if !self.registered.is_empty() {
            let _ = self.manager.unregister_all(&self.registered);
            self.registered.clear();
        }

        self.actions.clear();
    }
}

impl Drop for GlobalHotkeyListener {
    fn drop(&mut self) {
        self.unregister_all();
    }
}

/// Converts a parsed shortcut into a `global-hotkey` hotkey.
fn to_hotkey(shortcut: &Shortcut) -> Result<HotKey, HotkeyError> {
    let name = code_name(&shortcut.key).ok_or_else(|| HotkeyError::UnsupportedKey {
        key: shortcut.key.clone(),
    })?;
    let code = Code::from_str(&name).map_err(|_| HotkeyError::UnsupportedKey {
        key: shortcut.key.clone(),
    })?;

    let mut modifiers = Modifiers::empty();
    if shortcut.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if shortcut.alt {
        modifiers |= Modifiers::ALT;
    }
    if shortcut.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if shortcut.meta {
        modifiers |= Modifiers::META;
    }

    let modifiers = if modifiers.is_empty() {
        None
    } else {
        Some(modifiers)
    };

    Ok(HotKey::new(modifiers, code))
}

/// Maps a canonical key name onto the `keyboard-types` `Code` name.
fn code_name(key: &str) -> Option<String> {
    let named = match key {
        "Space" => "Space",
        "Tab" => "Tab",
        "Enter" => "Enter",
        "Escape" => "Escape",
        "Backspace" => "Backspace",
        "Delete" => "Delete",
        "Insert" => "Insert",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "PageUp",
        "PageDown" => "PageDown",
        "Up" => "Up",
        "Down" => "Down",
        "Left" => "Left",
        "Right" => "Right",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }

    if let Some(rest) = key.strip_prefix('F')
        && let Ok(number) = rest.parse::<u8>()
        && (1..=12).contains(&number)
    {
        return Some(key.to_owned());
    }

    let mut characters = key.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }

    let name = match character {
        'A'..='Z' => format!("Key{character}"),
        '0'..='9' => format!("Digit{character}"),
        '/' => "Slash".to_owned(),
        '\\' => "Backslash".to_owned(),
        ',' => "Comma".to_owned(),
        '.' => "Period".to_owned(),
        ';' => "Semicolon".to_owned(),
        '\'' => "Quote".to_owned(),
        '[' => "BracketLeft".to_owned(),
        ']' => "BracketRight".to_owned(),
        '-' => "Minus".to_owned(),
        '=' => "Equal".to_owned(),
        '`' => "Backquote".to_owned(),
        _ => return None,
    };

    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct MockState {
        registered: Vec<(SessionAction, Shortcut)>,
        register_calls: usize,
        unregister_calls: usize,
    }

    /// Records listener traffic through shared state so tests can inspect it
    /// after the guard (and listener) are dropped.
    #[derive(Clone)]
    struct MockShortcutListener {
        state: Arc<Mutex<MockState>>,
    }

    impl MockShortcutListener {
        fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    registered: Vec::new(),
                    register_calls: 0,
                    unregister_calls: 0,
                })),
            }
        }

        fn state(&self) -> std::sync::MutexGuard<'_, MockState> {
            self.state.lock().expect("mock listener state poisoned")
        }
    }

    impl ShortcutListener for MockShortcutListener {
        fn register(&mut self, shortcuts: &[(SessionAction, Shortcut)]) -> Result<(), HotkeyError> {
            let mut state = self.state.lock().expect("mock listener state poisoned");
            state.register_calls += 1;
            state.registered = shortcuts.to_vec();
            Ok(())
        }

        fn unregister_all(&mut self) {
            let mut state = self.state.lock().expect("mock listener state poisoned");
            state.unregister_calls += 1;
            state.registered.clear();
        }
    }

    fn shortcut(input: &str) -> Shortcut {
        Shortcut::parse(input).expect("shortcut parses")
    }

    fn default_shortcut_pairs() -> Vec<(SessionAction, Shortcut)> {
        default_bindings().into_iter().collect()
    }

    // T6.3 — parsing -----------------------------------------------------

    #[test]
    fn parses_common_shortcut_forms() {
        let parsed = shortcut("Ctrl+Shift+R");
        assert_eq!(
            parsed,
            Shortcut {
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
                key: "R".to_owned(),
            }
        );

        let spaced = shortcut("ctrl + alt + space");
        assert!(spaced.ctrl);
        assert!(spaced.alt);
        assert!(!spaced.shift);
        assert!(!spaced.meta);
        assert_eq!(spaced.key, "Space");

        let meta = shortcut("Super+/");
        assert!(meta.meta);
        assert!(!meta.ctrl);
        assert_eq!(meta.key, "/");

        let function = shortcut("F9");
        assert!(!function.ctrl && !function.alt && !function.shift && !function.meta);
        assert_eq!(function.key, "F9");
    }

    #[test]
    fn parsing_is_case_insensitive_but_canonical() {
        assert_eq!(shortcut("f9").key, "F9");
        assert_eq!(shortcut("CTRL+escape").to_string(), "Ctrl+Escape");
        assert_eq!(
            shortcut("shift+ctrl+alt+super+r").to_string(),
            "Ctrl+Alt+Shift+Super+R"
        );
        assert_eq!(shortcut("ctrl+shift+,").to_string(), "Ctrl+Shift+,");
    }

    #[test]
    fn display_round_trips_through_parse() {
        for input in [
            "Ctrl+Shift+R",
            "Ctrl+Alt+Shift+Super+Space",
            "Super+/",
            "F9",
            "Ctrl+Shift+,",
            "Alt+PageUp",
        ] {
            let parsed = shortcut(input);
            assert_eq!(parsed.to_string(), input);
            assert_eq!(shortcut(&parsed.to_string()), parsed);
        }
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(Shortcut::parse(""), Err(ShortcutError::Empty));
        assert_eq!(Shortcut::parse("   "), Err(ShortcutError::Empty));
    }

    #[test]
    fn unknown_modifier_is_rejected() {
        assert_eq!(
            Shortcut::parse("Hyper+R"),
            Err(ShortcutError::UnknownModifier("Hyper".to_owned()))
        );
        assert_eq!(
            Shortcut::parse("Ctrl+Hyper+R"),
            Err(ShortcutError::UnknownModifier("Hyper".to_owned()))
        );
    }

    #[test]
    fn missing_key_is_rejected() {
        assert_eq!(
            Shortcut::parse("Ctrl+Shift"),
            Err(ShortcutError::MissingKey("Ctrl+Shift".to_owned()))
        );
        assert_eq!(
            Shortcut::parse("Ctrl+"),
            Err(ShortcutError::MissingKey("Ctrl+".to_owned()))
        );
    }

    #[test]
    fn duplicate_modifier_is_rejected() {
        assert_eq!(
            Shortcut::parse("Ctrl+Ctrl+R"),
            Err(ShortcutError::DuplicateModifier("Ctrl".to_owned()))
        );
        // `Control` is the same modifier under its other name.
        assert_eq!(
            Shortcut::parse("Ctrl+Control+R"),
            Err(ShortcutError::DuplicateModifier("Control".to_owned()))
        );
    }

    #[test]
    fn unsupported_key_is_rejected() {
        assert_eq!(
            Shortcut::parse("Ctrl+F13"),
            Err(ShortcutError::UnknownKey("F13".to_owned()))
        );
        assert_eq!(
            Shortcut::parse("Ctrl+Hyper"),
            Err(ShortcutError::UnknownKey("Hyper".to_owned()))
        );
    }

    // T6.4 — bindings ----------------------------------------------------

    #[test]
    fn default_bindings_cover_every_action_without_conflicts() {
        let bindings = default_bindings();

        assert_eq!(bindings.len(), SessionAction::ALL.len());
        for action in SessionAction::ALL {
            assert!(bindings.contains_key(&action), "missing {action}");
        }
        assert!(conflicts(&bindings).is_empty());

        assert_eq!(
            bindings[&SessionAction::StartRecording].to_string(),
            "Ctrl+Shift+R"
        );
        assert_eq!(
            bindings[&SessionAction::PauseRecording].to_string(),
            "Ctrl+Shift+P"
        );
        assert_eq!(
            bindings[&SessionAction::ResumeRecording].to_string(),
            "Ctrl+Shift+O"
        );
        assert_eq!(
            bindings[&SessionAction::StopRecording].to_string(),
            "Ctrl+Shift+S"
        );
        assert_eq!(
            bindings[&SessionAction::NewTake].to_string(),
            "Ctrl+Shift+N"
        );
        assert_eq!(
            bindings[&SessionAction::ToggleHelp].to_string(),
            "Ctrl+Shift+H"
        );
        assert_eq!(
            bindings[&SessionAction::ToggleSettings].to_string(),
            "Ctrl+Shift+,"
        );
    }

    #[test]
    fn default_shortcuts_are_canonical() {
        for (action, shortcut) in default_bindings() {
            assert_eq!(
                Shortcut::parse(&shortcut.to_string()).expect("default parses"),
                shortcut,
                "default for {action} is not canonical"
            );
        }
    }

    #[test]
    fn every_default_shortcut_resolves_to_its_action() {
        let bindings = default_bindings();

        for (action, shortcut) in &bindings {
            assert_eq!(resolve(&bindings, shortcut), Some(*action));
        }
    }

    #[test]
    fn resolve_compares_modifiers_exactly_and_keys_case_insensitively() {
        let bindings = default_bindings();

        let lower_case = Shortcut {
            key: "r".to_owned(),
            ..shortcut("Ctrl+Shift+R")
        };
        assert_eq!(
            resolve(&bindings, &lower_case),
            Some(SessionAction::StartRecording)
        );

        // Same key, different modifiers.
        assert_eq!(resolve(&bindings, &shortcut("Ctrl+R")), None);
        assert_eq!(resolve(&bindings, &shortcut("Ctrl+Shift+Alt+R")), None);
    }

    #[test]
    fn resolve_unknown_shortcut_is_none() {
        assert_eq!(
            resolve(&default_bindings(), &shortcut("Ctrl+Shift+9")),
            None
        );
    }

    #[test]
    fn bind_replaces_the_same_actions_binding() {
        let mut bindings = default_bindings();
        let replacement = shortcut("Ctrl+Alt+9");

        bind(
            &mut bindings,
            SessionAction::StopRecording,
            replacement.clone(),
        )
        .expect("rebind same action");

        assert_eq!(bindings[&SessionAction::StopRecording], replacement);
        assert!(conflicts(&bindings).is_empty());
    }

    #[test]
    fn bind_rejects_a_duplicate_binding_and_keeps_the_map_unchanged() {
        let mut bindings = default_bindings();
        let before = bindings.clone();
        let duplicate = bindings[&SessionAction::StartRecording].clone();

        let error = bind(&mut bindings, SessionAction::NewTake, duplicate).expect_err("duplicate");

        assert_eq!(
            error,
            ShortcutError::Conflict {
                action: SessionAction::StartRecording,
                shortcut: shortcut("Ctrl+Shift+R"),
            }
        );
        assert_eq!(bindings, before);
    }

    #[test]
    fn conflicts_detects_two_actions_on_one_shortcut() {
        let mut bindings = default_bindings();
        let duplicate = bindings[&SessionAction::StartRecording].clone();
        bindings.insert(SessionAction::NewTake, duplicate);

        let conflicts = conflicts(&bindings);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, SessionAction::StartRecording);
        assert_eq!(conflicts[0].1, SessionAction::NewTake);
        assert_eq!(conflicts[0].2, shortcut("Ctrl+Shift+R"));
    }

    #[test]
    fn empty_settings_shortcuts_yield_the_defaults() {
        assert_eq!(
            bindings_from_settings(&Settings::default()).expect("empty shortcuts"),
            default_bindings()
        );
    }

    #[test]
    fn bindings_from_settings_round_trips_the_defaults() {
        let settings = Settings {
            shortcuts: default_bindings()
                .iter()
                .map(|(action, shortcut)| (action.as_str().to_owned(), shortcut.to_string()))
                .collect(),
            ..Settings::default()
        };

        assert_eq!(
            bindings_from_settings(&settings).expect("parse default shortcuts"),
            default_bindings()
        );
    }

    #[test]
    fn bindings_from_settings_overrides_one_action() {
        let settings = Settings {
            shortcuts: BTreeMap::from([("stopRecording".to_owned(), "Ctrl+Alt+9".to_owned())]),
            ..Settings::default()
        };

        let bindings = bindings_from_settings(&settings).expect("parse override");

        assert_eq!(
            bindings[&SessionAction::StopRecording],
            shortcut("Ctrl+Alt+9")
        );
        assert_eq!(
            bindings[&SessionAction::StartRecording],
            shortcut("Ctrl+Shift+R")
        );
    }

    #[test]
    fn bindings_from_settings_rejects_a_bad_shortcut() {
        let settings = Settings {
            shortcuts: BTreeMap::from([("startRecording".to_owned(), "Ctrl+".to_owned())]),
            ..Settings::default()
        };

        assert_eq!(
            bindings_from_settings(&settings),
            Err(ShortcutError::MissingKey("Ctrl+".to_owned()))
        );
    }

    #[test]
    fn bindings_from_settings_rejects_an_unknown_action() {
        let settings = Settings {
            shortcuts: BTreeMap::from([("dance".to_owned(), "Ctrl+Shift+D".to_owned())]),
            ..Settings::default()
        };

        assert_eq!(
            bindings_from_settings(&settings),
            Err(ShortcutError::UnknownAction("dance".to_owned()))
        );
    }

    #[test]
    fn bindings_from_settings_rejects_a_conflicting_override() {
        let settings = Settings {
            shortcuts: BTreeMap::from([("pauseRecording".to_owned(), "Ctrl+Shift+R".to_owned())]),
            ..Settings::default()
        };

        assert_eq!(
            bindings_from_settings(&settings),
            Err(ShortcutError::Conflict {
                action: SessionAction::StartRecording,
                shortcut: shortcut("Ctrl+Shift+R"),
            })
        );
    }

    #[test]
    fn session_actions_serialize_as_camel_case() {
        assert_eq!(
            serde_json::to_string(&SessionAction::StartRecording).expect("serialize action"),
            "\"startRecording\""
        );
        assert_eq!(
            serde_json::from_str::<SessionAction>("\"toggleSettings\"")
                .expect("deserialize action"),
            SessionAction::ToggleSettings
        );
    }

    // T6.5 — listener contract -------------------------------------------

    #[test]
    fn listener_guard_forwards_registrations() {
        let mock = MockShortcutListener::new();
        let observed = mock.clone();
        let mut guard = ListenerGuard::new(mock);
        let pairs = default_shortcut_pairs();

        guard.register(&pairs).expect("register through guard");

        let state = observed.state();
        assert_eq!(state.register_calls, 1);
        assert_eq!(state.registered, pairs);
        assert_eq!(state.unregister_calls, 0);
    }

    #[test]
    fn listener_guard_unregisters_on_drop() {
        let mock = MockShortcutListener::new();
        let observed = mock.clone();
        let mut guard = ListenerGuard::new(mock);
        guard
            .register(&default_shortcut_pairs())
            .expect("register through guard");

        drop(guard);

        let state = observed.state();
        assert_eq!(state.unregister_calls, 1);
        assert!(state.registered.is_empty());
    }

    #[test]
    fn listener_guard_explicit_unregister_is_idempotent() {
        let mock = MockShortcutListener::new();
        let observed = mock.clone();
        let mut guard = ListenerGuard::new(mock);

        guard.unregister_all();
        guard.unregister_all();
        drop(guard);

        assert_eq!(observed.state().unregister_calls, 3);
    }

    #[test]
    fn global_listener_implements_the_listener_contract() {
        fn assert_listener<T: ShortcutListener>() {}
        assert_listener::<GlobalHotkeyListener>();
    }

    #[test]
    fn shortcut_maps_to_an_os_hotkey_without_touching_the_display_server() {
        let hotkey = to_hotkey(&shortcut("Ctrl+Shift+R")).expect("convert shortcut");

        assert_eq!(hotkey.key, Code::KeyR);
        assert!(hotkey.mods.contains(Modifiers::CONTROL));
        assert!(hotkey.mods.contains(Modifiers::SHIFT));
        assert!(!hotkey.mods.contains(Modifiers::ALT));
        assert!(!hotkey.mods.contains(Modifiers::META));

        let meta_slash = to_hotkey(&shortcut("Super+/")).expect("convert meta shortcut");
        assert_eq!(meta_slash.key, Code::Slash);
        assert!(meta_slash.mods.contains(Modifiers::META));

        let f9 = to_hotkey(&shortcut("F9")).expect("convert function key");
        assert_eq!(f9.key, Code::F9);
        assert_eq!(f9.mods, Modifiers::empty());
    }

    #[test]
    fn unsupported_key_reports_an_error_instead_of_panicking() {
        let unsupported = Shortcut {
            ctrl: true,
            alt: false,
            shift: false,
            meta: false,
            key: "Nope".to_owned(),
        };

        assert!(matches!(
            to_hotkey(&unsupported),
            Err(HotkeyError::UnsupportedKey { key }) if key == "Nope"
        ));
    }
}
