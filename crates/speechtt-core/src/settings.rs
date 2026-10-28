//! Typed user settings and their on-disk JSON representation.
//!
//! [`Settings`] holds the active provider/model, per-provider API keys and the
//! raw shortcut strings (parsed by [`crate::hotkeys`], which keeps this module
//! free of shortcut semantics). API keys are [`SecretString`]s: they serialize
//! for persistence but never appear in `Debug` output.
//!
//! [`JsonFileStore`] persists settings as pretty-printed JSON. Saves are
//! atomic (write `<file>.tmp`, `sync_all`, rename over the target) and, on
//! Unix, owner-only (`0o600`). A failed save leaves the previous file intact.
//! Loading a missing file yields [`Settings::default`]; malformed JSON yields
//! [`SettingsError::Corrupt`] and never a partially populated value.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::protocol::SecretString;

/// Provider used until the user picks one.
pub const DEFAULT_PROVIDER: &str = "vercel-ai-gateway";

/// Legacy provider ids accepted from older settings files, mapped to the
/// canonical id. Keep in sync with `PROVIDER_ALIASES` in
/// `src/stt/registry.ts`.
pub const PROVIDER_ALIASES: [(&str, &str); 2] = [
    ("vercel", "vercel-ai-gateway"),
    ("gateway", "vercel-ai-gateway"),
];

/// Resolves a legacy provider id to its canonical form.
#[must_use]
fn canonical_provider_id(provider: &str) -> &str {
    PROVIDER_ALIASES
        .iter()
        .find(|(alias, _)| *alias == provider)
        .map_or(provider, |(_, canonical)| *canonical)
}

/// Directory name under the config root.
const APP_DIR: &str = "speechtt";

/// Name of the settings file.
const SETTINGS_FILE: &str = "settings.json";

/// User settings, persisted as JSON.
///
/// `deny_unknown_fields` turns stale or misspelled keys into a load error
/// instead of silently dropping user configuration.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Active STT provider id (for example `vercel`).
    pub provider: String,
    /// Active model id for the provider; empty until the user chooses one.
    pub model: String,
    /// Provider id -> API key. Serialized, but never exposed by `Debug`.
    pub api_keys: BTreeMap<String, SecretString>,
    /// Action name -> shortcut string (for example `startRecording` ->
    /// `Ctrl+Shift+R`). Parsed by [`crate::hotkeys::bindings_from_settings`].
    pub shortcuts: BTreeMap<String, String>,
    /// Paste the finished transcript into the focused field automatically.
    pub auto_paste: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_owned(),
            model: String::new(),
            api_keys: BTreeMap::new(),
            shortcuts: BTreeMap::new(),
            auto_paste: true,
        }
    }
}

impl fmt::Debug for Settings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Settings")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_keys", &"[REDACTED]")
            .field("shortcuts", &self.shortcuts)
            .field("auto_paste", &self.auto_paste)
            .finish()
    }
}

impl Settings {
    /// Rewrites legacy provider ids to their canonical form.
    ///
    /// Idempotent. A key stored under an alias is moved to the canonical id
    /// without re-encoding the secret; when both exist the canonical entry wins
    /// so a migrated file never loses the key the app was using.
    pub fn normalize_provider_ids(&mut self) {
        self.provider = canonical_provider_id(&self.provider).to_owned();
        for (alias, canonical) in PROVIDER_ALIASES {
            if let Some(secret) = self.api_keys.remove(alias) {
                self.api_keys.entry(canonical.to_owned()).or_insert(secret);
            }
        }
    }

    /// The stored key for `provider`, or `None` when absent or empty.
    ///
    /// Legacy provider ids resolve to the canonical id.
    #[must_use]
    pub fn api_key(&self, provider: &str) -> Option<&str> {
        self.api_keys
            .get(canonical_provider_id(provider))
            .map(SecretString::expose_secret)
            .filter(|key| !key.is_empty())
    }

    /// Stores `key` for `provider`. An empty key removes the entry, so a
    /// blank form field is indistinguishable from "no key".
    pub fn set_api_key(&mut self, provider: &str, key: impl Into<SecretString>) {
        let key = key.into();

        if key.expose_secret().is_empty() {
            self.remove_api_key(provider);
        } else {
            self.api_keys
                .insert(canonical_provider_id(provider).to_owned(), key);
        }
    }

    /// Removes any stored key for `provider`.
    pub fn remove_api_key(&mut self, provider: &str) {
        self.api_keys.remove(canonical_provider_id(provider));
    }

    /// A display mask for the stored key: `"sk-…78"` for keys of at least
    /// eight characters, otherwise a fixed `"••••"`. `None` when no key is
    /// stored.
    #[must_use]
    pub fn masked_api_key(&self, provider: &str) -> Option<String> {
        let key = self.api_key(provider)?;
        let length = key.chars().count();

        if length < 8 {
            return Some("••••".to_owned());
        }

        let first: String = key.chars().take(3).collect();
        let last: String = key.chars().skip(length - 2).collect();
        Some(format!("{first}…{last}"))
    }

    /// Checks the fields the core layer requires: a non-empty provider and
    /// model. Unknown provider ids are allowed because the Bun registry, not
    /// the core, owns provider discovery.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError::Invalid`] when the provider or model is empty
    /// or whitespace-only.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.provider.trim().is_empty() {
            return Err(SettingsError::Invalid(
                "provider must not be empty".to_owned(),
            ));
        }

        if self.model.trim().is_empty() {
            return Err(SettingsError::Invalid("model must not be empty".to_owned()));
        }

        Ok(())
    }
}

/// Why a settings operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    #[error("settings i/o failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("settings file at {path} is corrupt: {source}")]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to encode settings: {0}")]
    Encode(serde_json::Error),
    #[error("invalid settings: {0}")]
    Invalid(String),
}

/// Persistence seam for settings, injectable in tests and in the UI.
pub trait SettingsStore: Send {
    /// Loads settings, returning defaults when no file exists.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError::Io`] for unreadable files and
    /// [`SettingsError::Corrupt`] for malformed or structurally invalid JSON.
    fn load(&self) -> Result<Settings, SettingsError>;

    /// Persists settings atomically.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError`] when the directory, temporary file, write,
    /// sync or rename fails. The previous file is left untouched.
    fn save(&self, settings: &Settings) -> Result<(), SettingsError>;

    /// Location of the backing file.
    fn path(&self) -> &Path;
}

/// A [`SettingsStore`] backed by one JSON file.
pub struct JsonFileStore {
    path: PathBuf,
}

impl JsonFileStore {
    /// Creates a store for an explicit path (the test and UI seam).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_CONFIG_HOME/speechtt/settings.json`, else
    /// `$HOME/.config/speechtt/settings.json`, else
    /// `<temp>/speechtt/settings.json`.
    #[must_use]
    pub fn default_path() -> PathBuf {
        resolve_default_path(
            env::var_os("XDG_CONFIG_HOME"),
            env::var_os("HOME"),
            env::temp_dir(),
        )
    }
}

impl SettingsStore for JsonFileStore {
    fn load(&self) -> Result<Settings, SettingsError> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let mut settings: Settings =
                    serde_json::from_slice(&bytes).map_err(|source| SettingsError::Corrupt {
                        path: self.path.clone(),
                        source,
                    })?;
                settings.normalize_provider_ids();
                Ok(settings)
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(source) => Err(SettingsError::Io {
                path: self.path.clone(),
                source,
            }),
        }
    }

    fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        // Persist canonical ids only, so a migrated file never regress.
        let mut settings = settings.clone();
        settings.normalize_provider_ids();
        let mut contents = serde_json::to_vec_pretty(&settings).map_err(SettingsError::Encode)?;
        contents.push(b'\n');

        write_atomic(&self.path, &contents)
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

/// Resolution of the config root, kept pure so tests never mutate process
/// environment variables.
fn resolve_default_path(xdg: Option<OsString>, home: Option<OsString>, temp: PathBuf) -> PathBuf {
    let root = non_empty_path(xdg)
        .or_else(|| non_empty_path(home).map(|home| home.join(".config")))
        .unwrap_or(temp);

    root.join(APP_DIR).join(SETTINGS_FILE)
}

/// Treats an empty environment variable as unset.
fn non_empty_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Writes `contents` to `path` via a sibling temporary file and a rename.
///
/// The temporary file is removed on every failure path, so a failed save
/// never leaves partial JSON behind and never replaces the previous file.
fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), SettingsError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| SettingsError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let temporary = temporary_path(path);
    let result = write_temporary_file(&temporary, path, contents);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }

    result
}

/// `<file>.tmp`, next to the target so the final rename stays atomic.
fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

fn write_temporary_file(
    temporary: &Path,
    target: &Path,
    contents: &[u8],
) -> Result<(), SettingsError> {
    let mut file = create_private_file(temporary).map_err(|source| SettingsError::Io {
        path: temporary.to_path_buf(),
        source,
    })?;

    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|source| SettingsError::Io {
            path: temporary.to_path_buf(),
            source,
        })?;

    drop(file);

    fs::rename(temporary, target).map_err(|source| SettingsError::Io {
        path: target.to_path_buf(),
        source,
    })
}

/// Creates the temporary file with owner-only permissions from the start.
fn create_private_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let file = options.open(path)?;

    // `mode` only applies at creation, so fix up a stale temporary file left
    // by an interrupted write.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique directory under the OS temp dir; removed on drop.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);

            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "speechtt-settings-test-{label}-{}-{nanos}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp test directory");
            Self { path }
        }

        fn join(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn sample_settings() -> Settings {
        Settings {
            provider: DEFAULT_PROVIDER.to_owned(),
            model: "mistral/voxtral-mini-transcribe-3".to_owned(),
            api_keys: BTreeMap::from([(
                DEFAULT_PROVIDER.to_owned(),
                SecretString::new("sk-live-secret-123456"),
            )]),
            shortcuts: BTreeMap::from([("startRecording".to_owned(), "Ctrl+Shift+R".to_owned())]),
            auto_paste: false,
        }
    }

    // T6.1 — model -------------------------------------------------------

    #[test]
    fn default_settings_are_usable_but_need_a_model() {
        let settings = Settings::default();

        assert_eq!(settings.provider, DEFAULT_PROVIDER);
        assert_eq!(settings.provider, "vercel-ai-gateway");
        assert!(settings.model.is_empty());
        assert!(settings.api_keys.is_empty());
        assert!(settings.shortcuts.is_empty());
        assert!(settings.auto_paste);

        // A fresh install has no model yet, so validation fails until the
        // user (or the provider registry) supplies one.
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::Invalid(message)) if message.contains("model")
        ));

        let chosen = Settings {
            model: "mistral/voxtral-mini-transcribe-3".to_owned(),
            ..Settings::default()
        };
        assert!(chosen.validate().is_ok());
    }

    #[test]
    fn api_keys_are_addressed_by_provider() {
        let mut settings = Settings::default();

        settings.set_api_key("vercel", "sk-live-secret-123456");
        settings.set_api_key("mistral", "msk-other-secret");

        assert_eq!(settings.api_key("vercel"), Some("sk-live-secret-123456"));
        assert_eq!(settings.api_key("mistral"), Some("msk-other-secret"));
        assert_eq!(settings.api_key("local"), None);

        settings.remove_api_key("vercel");
        assert_eq!(settings.api_key("vercel"), None);
        assert_eq!(settings.api_key("mistral"), Some("msk-other-secret"));
    }

    #[test]
    fn empty_api_key_is_absent() {
        let mut settings = Settings::default();

        settings.set_api_key("vercel", "");
        assert_eq!(settings.api_key("vercel"), None);
        assert!(!settings.api_keys.contains_key("vercel"));
        assert_eq!(settings.masked_api_key("vercel"), None);

        settings.set_api_key("vercel", "sk-12345678");
        settings.set_api_key("vercel", "");
        assert_eq!(settings.api_key("vercel"), None);
        assert!(!settings.api_keys.contains_key("vercel"));
    }

    #[test]
    fn masked_api_key_never_reveals_the_middle() {
        let mut settings = Settings::default();

        settings.set_api_key("vercel", "sk-live-123456");
        assert_eq!(settings.masked_api_key("vercel").as_deref(), Some("sk-…56"));

        settings.set_api_key("vercel", "abcdefgh");
        assert_eq!(settings.masked_api_key("vercel").as_deref(), Some("abc…gh"));

        settings.set_api_key("vercel", "short");
        assert_eq!(settings.masked_api_key("vercel").as_deref(), Some("••••"));

        settings.remove_api_key("vercel");
        assert_eq!(settings.masked_api_key("vercel"), None);
    }

    #[test]
    fn settings_round_trip_through_serde() {
        let settings = sample_settings();

        let json = serde_json::to_string(&settings).expect("serialize settings");
        // Persistence is the one place where key material is required.
        assert!(json.contains("sk-live-secret-123456"));

        let restored: Settings = serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(restored, settings);
        assert_eq!(restored.api_key("vercel"), Some("sk-live-secret-123456"));
    }

    #[test]
    fn debug_output_redacts_api_keys() {
        let mut settings = sample_settings();
        settings.set_api_key("mistral", "msk-another-live-secret");

        let debug = format!("{settings:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("sk-live-secret-123456"));
        assert!(!debug.contains("msk-another-live-secret"));
        assert!(debug.contains("vercel"));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let result = serde_json::from_str::<Settings>(
            r#"{"provider":"vercel","model":"m","surprise":true}"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_blank_provider_and_model() {
        let no_provider = Settings {
            provider: "  ".to_owned(),
            model: "m".to_owned(),
            ..Settings::default()
        };
        assert!(matches!(
            no_provider.validate(),
            Err(SettingsError::Invalid(_))
        ));

        let no_model = Settings {
            model: String::new(),
            ..Settings::default()
        };
        assert!(matches!(
            no_model.validate(),
            Err(SettingsError::Invalid(_))
        ));
    }

    #[test]
    fn validate_allows_unknown_provider_ids() {
        let settings = Settings {
            provider: "future-provider".to_owned(),
            model: "future-model".to_owned(),
            ..Settings::default()
        };

        assert!(settings.validate().is_ok());
    }

    // T6.2 — store -------------------------------------------------------

    #[test]
    fn load_missing_file_returns_default() {
        let dir = TempDir::new("missing");
        let store = JsonFileStore::new(dir.join("settings.json"));

        assert_eq!(
            store.load().expect("missing file loads"),
            Settings::default()
        );
        assert_eq!(store.path(), dir.join("settings.json"));
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("round-trip");
        let store = JsonFileStore::new(dir.join("settings.json"));
        let settings = sample_settings();

        store.save(&settings).expect("save settings");
        assert_eq!(store.load().expect("load settings"), settings);
    }

    // T10.1 — key integrity ----------------------------------------------

    #[test]
    fn mixed_case_api_key_round_trips_byte_for_byte() {
        let dir = TempDir::new("mixed-case-key");
        let store = JsonFileStore::new(dir.join("settings.json"));
        let key = "vck_AbCdEfGhIjKlMnOpQrStUvWxYzH0123456789";
        let mut settings = Settings::default();

        settings.set_api_key("vercel", key);
        store.save(&settings).expect("save mixed-case key");
        let loaded = store.load().expect("load mixed-case key");

        assert!(
            loaded.api_key("vercel") == Some(key),
            "the stored key differs after the round trip"
        );
        assert!(
            loaded
                .api_key("vercel")
                .is_some_and(|stored| stored.as_bytes() == key.as_bytes()),
            "every byte, including case, must survive the round trip"
        );
        assert_eq!(loaded, settings);
    }

    #[test]
    fn load_fills_missing_fields_with_defaults() {
        let dir = TempDir::new("partial");
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"provider":"mistral"}"#).expect("write partial settings");
        let store = JsonFileStore::new(&path);

        let settings = store.load().expect("partial settings load");

        assert_eq!(settings.provider, "mistral");
        assert_eq!(settings.model, "");
        assert!(settings.auto_paste);
    }

    #[test]
    fn corrupt_json_returns_corrupt_and_leaves_the_file_untouched() {
        let dir = TempDir::new("corrupt");
        let path = dir.join("settings.json");
        fs::write(&path, "{ this is not json").expect("write corrupt settings");
        let store = JsonFileStore::new(&path);

        let error = store.load().expect_err("corrupt settings must fail");

        assert!(matches!(
            error,
            SettingsError::Corrupt { ref path, .. } if path.as_path() == store.path()
        ));
        assert_eq!(
            fs::read_to_string(&path).expect("read corrupt settings"),
            "{ this is not json"
        );
    }

    #[test]
    fn unknown_field_in_file_returns_corrupt() {
        let dir = TempDir::new("unknown-field");
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"provider":"vercel","legacy":1}"#).expect("write settings");
        let store = JsonFileStore::new(&path);

        assert!(matches!(
            store.load().expect_err("unknown field must fail"),
            SettingsError::Corrupt { .. }
        ));
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = TempDir::new("parents");
        let store = JsonFileStore::new(dir.join("nested/config/settings.json"));
        let settings = sample_settings();

        store.save(&settings).expect("save into new directories");

        assert!(store.path().is_file());
        assert_eq!(store.load().expect("load settings"), settings);
    }

    #[test]
    fn failed_save_keeps_the_previous_file_intact() {
        let dir = TempDir::new("atomic");
        let store = JsonFileStore::new(dir.join("settings.json"));
        let original = sample_settings();
        store.save(&original).expect("save original");

        // A directory where the temporary file should go makes the write fail
        // after the target already exists.
        fs::create_dir(dir.join("settings.json.tmp")).expect("block temporary path");

        let updated = Settings {
            model: "other/model".to_owned(),
            ..original.clone()
        };
        let error = store
            .save(&updated)
            .expect_err("blocked temp path must fail");

        assert!(matches!(error, SettingsError::Io { .. }));
        assert_eq!(store.load().expect("original still loads"), original);
    }

    #[test]
    fn failed_save_does_not_create_a_file() {
        let dir = TempDir::new("blocked-parent");
        let blocked_parent = dir.join("blocked");
        fs::write(&blocked_parent, b"not a directory").expect("create blocking file");
        let store = JsonFileStore::new(blocked_parent.join("settings.json"));

        let error = store
            .save(&sample_settings())
            .expect_err("file parent must fail");

        assert!(matches!(error, SettingsError::Io { .. }));
        assert!(!blocked_parent.join("settings.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("mode");
        let store = JsonFileStore::new(dir.join("settings.json"));
        store.save(&sample_settings()).expect("save settings");

        let mode = fs::metadata(store.path())
            .expect("settings metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn saved_json_is_pretty_printed_in_stable_field_order() {
        let dir = TempDir::new("pretty");
        let store = JsonFileStore::new(dir.join("settings.json"));
        store.save(&sample_settings()).expect("save settings");

        let json = fs::read_to_string(store.path()).expect("read saved settings");

        assert!(json.ends_with("}\n"));
        assert!(json.contains("\n  \"provider\": \"vercel-ai-gateway\""));
        let provider = json.find("\"provider\"").expect("provider field");
        let model = json.find("\"model\"").expect("model field");
        let api_keys = json.find("\"api_keys\"").expect("api_keys field");
        let shortcuts = json.find("\"shortcuts\"").expect("shortcuts field");
        let auto_paste = json.find("\"auto_paste\"").expect("auto_paste field");
        assert!(provider < model);
        assert!(model < api_keys);
        assert!(api_keys < shortcuts);
        assert!(shortcuts < auto_paste);
    }

    #[test]
    fn default_path_prefers_xdg_config_home() {
        let path = resolve_default_path(
            Some(OsString::from("/tmp/xdg-config")),
            Some(OsString::from("/home/example")),
            PathBuf::from("/tmp"),
        );

        assert_eq!(
            path,
            PathBuf::from("/tmp/xdg-config/speechtt/settings.json")
        );
    }

    #[test]
    fn default_path_falls_back_to_home_config() {
        let path = resolve_default_path(
            Some(OsString::new()),
            Some(OsString::from("/home/example")),
            PathBuf::from("/tmp"),
        );

        assert_eq!(
            path,
            PathBuf::from("/home/example/.config/speechtt/settings.json")
        );
    }

    #[test]
    fn default_path_falls_back_to_temp_directory() {
        let path = resolve_default_path(None, None, PathBuf::from("/tmp"));

        assert_eq!(path, PathBuf::from("/tmp/speechtt/settings.json"));
    }

    // T11.6 — provider id migration --------------------------------------

    #[test]
    fn legacy_provider_ids_are_migrated_on_load() {
        let dir = TempDir::new("legacy-provider");
        let store = JsonFileStore::new(dir.join("settings.json"));
        let key = "vck_AbCdEfGhIjKlMnOpQrStUvWxYzH0123456789";
        fs::write(
            store.path(),
            format!(
                r#"{{"provider":"vercel","model":"openai/gpt-4o-transcribe","api_keys":{{"vercel":"{key}"}}}}"#,
            ),
        )
        .expect("write legacy settings");

        let loaded = store.load().expect("load legacy settings");

        assert_eq!(loaded.provider, "vercel-ai-gateway");
        assert_eq!(loaded.api_key("vercel-ai-gateway"), Some(key));
        assert_eq!(loaded.api_key("vercel"), Some(key));
        assert!(
            loaded
                .api_key("vercel-ai-gateway")
                .is_some_and(|stored| stored.as_bytes() == key.as_bytes()),
            "the migrated key must be byte-identical"
        );
        assert_eq!(loaded.api_keys.len(), 1);
        assert!(!loaded.api_keys.contains_key("vercel"));
    }

    #[test]
    fn the_gateway_alias_is_migrated_too() {
        let mut settings = Settings {
            provider: "gateway".to_owned(),
            model: "openai/gpt-4o-transcribe".to_owned(),
            ..Settings::default()
        };
        settings
            .api_keys
            .insert("gateway".to_owned(), SecretString::new("sk-gateway-secret"));

        settings.normalize_provider_ids();

        assert_eq!(settings.provider, "vercel-ai-gateway");
        assert_eq!(
            settings.api_key("vercel-ai-gateway"),
            Some("sk-gateway-secret")
        );
        assert_eq!(settings.api_keys.len(), 1);
    }

    #[test]
    fn the_canonical_key_wins_when_both_entries_exist() {
        let mut settings = Settings {
            provider: "vercel".to_owned(),
            ..Settings::default()
        };
        settings
            .api_keys
            .insert("vercel".to_owned(), SecretString::new("sk-legacy"));
        settings
            .api_keys
            .insert(DEFAULT_PROVIDER.to_owned(), SecretString::new("sk-current"));

        settings.normalize_provider_ids();

        assert_eq!(settings.api_key("vercel-ai-gateway"), Some("sk-current"));
        assert_eq!(settings.api_keys.len(), 1);
    }

    #[test]
    fn normalization_is_idempotent_and_leaves_other_keys_alone() {
        let mut settings = Settings {
            provider: "vercel".to_owned(),
            ..Settings::default()
        };
        settings
            .api_keys
            .insert("vercel".to_owned(), SecretString::new("sk-legacy"));
        settings
            .api_keys
            .insert("mistral".to_owned(), SecretString::new("msk-other"));

        settings.normalize_provider_ids();
        let once = settings.clone();
        settings.normalize_provider_ids();

        assert_eq!(settings, once);
        assert_eq!(settings.api_key("mistral"), Some("msk-other"));
        assert_eq!(settings.model, once.model);
    }

    #[test]
    fn saving_a_legacy_settings_value_writes_the_canonical_id() {
        let dir = TempDir::new("migrate-on-save");
        let store = JsonFileStore::new(dir.join("settings.json"));
        let key = "vck_MiXeDcAsE0123456789";
        let mut settings = Settings {
            provider: "vercel".to_owned(),
            model: "openai/gpt-4o-transcribe".to_owned(),
            ..Settings::default()
        };
        settings
            .api_keys
            .insert("vercel".to_owned(), SecretString::new(key));

        store.save(&settings).expect("save legacy settings");

        let json = fs::read_to_string(store.path()).expect("read saved settings");
        assert!(json.contains("\"provider\": \"vercel-ai-gateway\""));
        assert!(json.contains(&format!("\"vercel-ai-gateway\": \"{key}\"")));
        assert!(!json.contains("\"vercel\":"));
        assert_eq!(store.load().expect("reload").api_key("vercel"), Some(key));
    }
}
