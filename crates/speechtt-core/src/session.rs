//! Session state machine: nothing records until the session is fully
//! configured.
//!
//! A take needs three parameters: audio source, AI provider, and model.
//! [`SessionMachine`] owns that configuration and derives its state from it:
//!
//! ```text
//!        configure all three           start                pause
//! Blank ───────────────────▶ Ready ──────────▶ Recording ⇄──────────▶ Paused
//!   ▲                          ▲                   │  ▲               │
//!   │  remove a parameter      │   stop            │  └── resume ─────┘
//!   └──────────────────────────┘◀──────────────────┘        (stop also allowed)
//! ```
//!
//! - Setup changes are allowed only while `Blank`/`Ready`; a take freezes
//!   the configuration.
//! - `stop` returns the finished session to the caller (which owns the
//!   transcript and the save/paste/copy actions), then clears the whole
//!   configuration and returns to `Blank`. The next take starts from the mode
//!   chooser, which re-derives the mode, provider and model.
//! - `reset` clears an idle machine; it backs the UI's "change mode" action.

/// Transport state. `Blank` and `Ready` are derived from configuration
/// completeness; `Recording` and `Paused` are explicit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionState {
    /// At least one required parameter is missing.
    #[default]
    Blank,
    /// All required parameters are present; recording may start.
    Ready,
    Recording,
    Paused,
}

impl SessionState {
    /// True while a take is active (setup changes are frozen).
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, SessionState::Recording | SessionState::Paused)
    }
}

/// The three parameters a take requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionField {
    AudioSource,
    Provider,
    Model,
}

/// Session configuration. `None` means "not chosen yet"; the setters reject
/// whitespace-only values, storing `None` instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionConfig {
    pub audio_source: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl SessionConfig {
    /// Required parameters still missing, in declaration order.
    #[must_use]
    pub fn missing(&self) -> Vec<SessionField> {
        let mut missing = Vec::new();
        if self.audio_source.is_none() {
            missing.push(SessionField::AudioSource);
        }
        if self.provider.is_none() {
            missing.push(SessionField::Provider);
        }
        if self.model.is_none() {
            missing.push(SessionField::Model);
        }
        missing
    }

    /// True when every required parameter is present.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.missing().is_empty()
    }
}

/// The take handed back by [`SessionMachine::stop`]. The caller owns the
/// transcript; this carries the identity needed to label or paste it. A saved
/// file names the take — the machine never does.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FinishedSession {
    pub audio_source: String,
    pub provider: String,
    pub model: String,
}

/// Why an action was refused. Refusals never mutate the machine.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// `start` while configuration is incomplete.
    #[error("session is not ready: still missing {missing:?}")]
    NotReady { missing: Vec<SessionField> },
    /// Transport action not valid for the current state.
    #[error("action is not valid while {from:?}")]
    InvalidTransition { from: SessionState },
    /// Setup change attempted while a take is active.
    #[error("session setup is frozen while {state:?}")]
    Frozen { state: SessionState },
}

/// Owns the session configuration and its transport state.
#[derive(Debug, Clone, Default)]
pub struct SessionMachine {
    config: SessionConfig,
    state: SessionState,
}

impl SessionMachine {
    /// A machine with nothing configured (`Blank`).
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn state(&self) -> SessionState {
        self.state
    }

    #[must_use]
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Required parameters still missing, in declaration order. Empty only
    /// when the machine is `Ready` (or a take is active).
    #[must_use]
    pub fn missing(&self) -> Vec<SessionField> {
        self.config.missing()
    }

    pub fn set_audio_source(&mut self, value: impl Into<String>) -> Result<(), SessionError> {
        let value = value.into();
        self.guard_idle()?;
        self.config.audio_source = non_empty(value);
        self.refresh();
        Ok(())
    }

    pub fn set_provider(&mut self, value: impl Into<String>) -> Result<(), SessionError> {
        let value = value.into();
        self.guard_idle()?;
        self.config.provider = non_empty(value);
        self.refresh();
        Ok(())
    }

    pub fn set_model(&mut self, value: impl Into<String>) -> Result<(), SessionError> {
        let value = value.into();
        self.guard_idle()?;
        self.config.model = non_empty(value);
        self.refresh();
        Ok(())
    }

    /// Unset one parameter (settings "clear" action).
    pub fn clear(&mut self, field: SessionField) -> Result<(), SessionError> {
        self.guard_idle()?;
        match field {
            SessionField::AudioSource => self.config.audio_source = None,
            SessionField::Provider => self.config.provider = None,
            SessionField::Model => self.config.model = None,
        }
        self.refresh();
        Ok(())
    }

    /// Begins a take. Requires `Ready`.
    pub fn start(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Ready => {
                self.state = SessionState::Recording;
                Ok(())
            }
            SessionState::Blank => Err(SessionError::NotReady {
                missing: self.config.missing(),
            }),
            from => Err(SessionError::InvalidTransition { from }),
        }
    }

    pub fn pause(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Recording => {
                self.state = SessionState::Paused;
                Ok(())
            }
            from => Err(SessionError::InvalidTransition { from }),
        }
    }

    pub fn resume(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Paused => {
                self.state = SessionState::Recording;
                Ok(())
            }
            from => Err(SessionError::InvalidTransition { from }),
        }
    }

    /// Ends the take (from Recording or Paused), hands the finished session
    /// to the caller, then clears the whole configuration and returns to
    /// `Blank`.
    pub fn stop(&mut self) -> Result<FinishedSession, SessionError> {
        if !self.state.is_active() {
            return Err(SessionError::InvalidTransition { from: self.state });
        }

        let config = &self.config;
        let finished = FinishedSession {
            audio_source: config.audio_source.clone().unwrap_or_default(),
            provider: config.provider.clone().unwrap_or_default(),
            model: config.model.clone().unwrap_or_default(),
        };

        // The next take starts from the mode chooser and re-derives everything.
        self.config = SessionConfig::default();
        self.state = SessionState::Blank;
        Ok(finished)
    }

    /// Clears the configuration and returns to `Blank`. Refuses while a take is
    /// active (use `stop` for that). Backs the UI's "change mode" action.
    pub fn reset(&mut self) -> Result<(), SessionError> {
        if self.state.is_active() {
            return Err(SessionError::InvalidTransition { from: self.state });
        }
        self.config = SessionConfig::default();
        self.state = SessionState::Blank;
        Ok(())
    }

    fn guard_idle(&self) -> Result<(), SessionError> {
        if self.state.is_active() {
            Err(SessionError::Frozen { state: self.state })
        } else {
            Ok(())
        }
    }

    /// `Blank`/`Ready` are pure functions of configuration completeness.
    fn refresh(&mut self) {
        if !self.state.is_active() {
            self.state = if self.config.is_complete() {
                SessionState::Ready
            } else {
                SessionState::Blank
            };
        }
    }
}

/// Trim and reject empty strings: a blank name is not a name.
fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
