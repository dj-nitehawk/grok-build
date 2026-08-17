//! Voice runtime surface used by the pager.
//!
//! When feature `voice` is on, this re-exports [`xai_grok_voice`]. When off,
//! it provides compile-time stubs so call sites keep stable paths without
//! linking the voice crate (mic capture / STT pipeline).

#![allow(dead_code)]

#[cfg(feature = "voice")]
pub use xai_grok_voice::*;

#[cfg(not(feature = "voice"))]
mod stub {
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::Arc;

    pub const AUDIO_SUPPORTED: bool = false;
    pub const MIC_CAPTURE_SUBCOMMAND: &str = "__mic-capture";
    pub const STT_LANGUAGE_AUTO: &str = "auto";
    pub const STT_LANGUAGE_DEFAULT: &str = "en";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SttLanguage {
        pub code: &'static str,
        pub name: &'static str,
    }

    // Catalog kept in lockstep with xai-grok-voice so settings UI stays complete
    // even when the voice pipeline is compiled out.
    pub const STT_LANGUAGES: &[SttLanguage] = &[
    SttLanguage {
        code: "ar",
        name: "Arabic",
    },
    SttLanguage {
        code: "cs",
        name: "Czech",
    },
    SttLanguage {
        code: "da",
        name: "Danish",
    },
    SttLanguage {
        code: "nl",
        name: "Dutch",
    },
    SttLanguage {
        code: "en",
        name: "English",
    },
    SttLanguage {
        code: "fil",
        name: "Filipino",
    },
    SttLanguage {
        code: "fr",
        name: "French",
    },
    SttLanguage {
        code: "de",
        name: "German",
    },
    SttLanguage {
        code: "hi",
        name: "Hindi",
    },
    SttLanguage {
        code: "id",
        name: "Indonesian",
    },
    SttLanguage {
        code: "it",
        name: "Italian",
    },
    SttLanguage {
        code: "ja",
        name: "Japanese",
    },
    SttLanguage {
        code: "ko",
        name: "Korean",
    },
    SttLanguage {
        code: "mk",
        name: "Macedonian",
    },
    SttLanguage {
        code: "ms",
        name: "Malay",
    },
    SttLanguage {
        code: "fa",
        name: "Persian",
    },
    SttLanguage {
        code: "pl",
        name: "Polish",
    },
    SttLanguage {
        code: "pt",
        name: "Portuguese",
    },
    SttLanguage {
        code: "ro",
        name: "Romanian",
    },
    SttLanguage {
        code: "ru",
        name: "Russian",
    },
    SttLanguage {
        code: "es",
        name: "Spanish",
    },
    SttLanguage {
        code: "sv",
        name: "Swedish",
    },
    SttLanguage {
        code: "th",
        name: "Thai",
    },
    SttLanguage {
        code: "tr",
        name: "Turkish",
    },
    SttLanguage {
        code: "vi",
        name: "Vietnamese",
    },
    ];

    /// Look up a catalog entry by exact (case-sensitive) code.
    pub fn stt_language_by_code(code: &str) -> Option<&'static SttLanguage> {
    STT_LANGUAGES.iter().find(|l| l.code == code)
}

/// Map a user/config string to a catalog code or [`STT_LANGUAGE_AUTO`].
///
/// - `None` / blank / unknown → [`STT_LANGUAGE_DEFAULT`] (`en`)
/// - `auto` (any case) → [`STT_LANGUAGE_AUTO`]
/// - Exact catalog code (any case) → that code
/// - BCP-47 / locale forms (`en-US`, `pt_BR.UTF-8`) → primary subtag when supported
/// - Common aliases: `tl` → `fil` (Tagalog → Filipino)
pub fn canonicalize_stt_language(value: Option<&str>) -> &'static str {
    let raw = value.unwrap_or_default().trim();
    if raw.is_empty() {
        return STT_LANGUAGE_DEFAULT;
    }
    if raw.eq_ignore_ascii_case(STT_LANGUAGE_AUTO) {
        return STT_LANGUAGE_AUTO;
    }

    if let Some(code) = match_supported_code(raw) {
        return code;
    }

    // Primary subtag of BCP-47 / POSIX locales.
    let primary = primary_language_subtag(raw);
    if let Some(code) = match_supported_code(primary) {
        return code;
    }
    if let Some(aliased) = alias_to_supported(primary) {
        return aliased;
    }

    STT_LANGUAGE_DEFAULT
}

/// Concrete language code to send on the STT wire.
///
/// Resolves [`STT_LANGUAGE_AUTO`] from the process locale; never returns `auto`.
pub fn language_for_api(stored: &str) -> &'static str {
    let canonical = canonicalize_stt_language(Some(stored));
    if canonical == STT_LANGUAGE_AUTO {
        system_stt_language().unwrap_or(STT_LANGUAGE_DEFAULT)
    } else {
        canonical
    }
}

/// Best-effort system locale → supported STT code (`None` if unset/unsupported).
///
/// POSIX precedence, treating set-but-empty vars as unset (an empty `LC_ALL`
/// must not mask a usable `LANG`).
fn system_stt_language() -> Option<&'static str> {
    let loc = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))?;
    if loc.eq_ignore_ascii_case("C") || loc.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    let primary = primary_language_subtag(&loc);
    match_supported_code(primary).or_else(|| alias_to_supported(primary))
}

fn primary_language_subtag(raw: &str) -> &str {
    raw.split(['_', '-', '.']).next().unwrap_or("").trim()
}

fn match_supported_code(raw: &str) -> Option<&'static str> {
    STT_LANGUAGES
        .iter()
        .map(|l| l.code)
        .find(|&code| raw.eq_ignore_ascii_case(code))
}

/// Map common non-catalog primaries onto a supported code.
fn alias_to_supported(primary: &str) -> Option<&'static str> {
    // Tagalog (`tl`) is the usual system locale; API uses Filipino (`fil`).
    if primary.eq_ignore_ascii_case("tl") {
        return Some("fil");
    }
    None
}


    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    #[serde(default)]
    pub struct VoiceConfig {
        pub api_base: String,
        pub stt_ws_path: String,
        pub language: String,
        pub sample_rate: u32,
        pub stt_endpointing_ms: u32,
        pub stt_interim_results: bool,
        #[serde(skip)]
        pub client_identifier: String,
        #[serde(skip)]
        pub user_agent: String,
    }

    impl Default for VoiceConfig {
        fn default() -> Self {
            Self {
                api_base: "https://api.x.ai".into(),
                stt_ws_path: "/v1/stt".into(),
                language: STT_LANGUAGE_DEFAULT.into(),
                sample_rate: 16_000,
                stt_endpointing_ms: 400,
                stt_interim_results: true,
                client_identifier: String::new(),
                user_agent: String::new(),
            }
        }
    }

    impl VoiceConfig {
        pub fn from_config_table(
            _root: &toml::Table,
            _resolved_endpoints_base: Option<&str>,
        ) -> Self {
            Self::default()
        }
    }

    #[derive(Debug, Clone)]
    pub enum VoiceError {
        Config(String),
        Stt(String),
        Auth(String),
        WebSocket(String),
    }

    impl std::fmt::Display for VoiceError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Config(m) => write!(f, "configuration: {m}"),
                Self::Stt(m) => write!(f, "STT: {m}"),
                Self::Auth(m) => write!(f, "auth: {m}"),
                Self::WebSocket(m) => write!(f, "WebSocket: {m}"),
            }
        }
    }

    impl std::error::Error for VoiceError {}

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum VoiceEvent {
        InterimTranscript { text: String },
        UtteranceFinal { text: String },
        Error {
            message: String,
            hint: Option<String>,
        },
    }

    #[derive(Debug)]
    pub enum VoiceCommand {
        PttPress,
        PttRelease,
        Shutdown,
    }

    pub trait VoiceAuthProvider: std::fmt::Debug + Send + Sync + 'static {
        fn bearer(&self) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>>;
    }

    pub type SharedVoiceAuth = Arc<dyn VoiceAuthProvider>;

    pub struct StaticVoiceAuth(pub String);

    impl std::fmt::Debug for StaticVoiceAuth {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("StaticVoiceAuth")
                .field(&"<redacted>")
                .finish()
        }
    }

    impl VoiceAuthProvider for StaticVoiceAuth {
        fn bearer(&self) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
            Box::pin(ready(Some(self.0.clone())))
        }
    }

    impl StaticVoiceAuth {
        pub fn shared(key: impl Into<String>) -> Option<SharedVoiceAuth> {
            let key = key.into().trim().to_string();
            if key.is_empty() {
                return None;
            }
            Some(Arc::new(Self(key)))
        }
    }

    #[derive(Debug, Clone)]
    pub struct InputDeviceInfo {
        pub name: String,
        pub detail: String,
    }

    pub fn input_device_info() -> Result<InputDeviceInfo, VoiceError> {
        Err(VoiceError::Config(
            "Voice support is not compiled into this build (missing feature `voice`)".into(),
        ))
    }

    pub fn maybe_run_capture_subprocess() -> Option<i32> {
        None
    }

    pub async fn run_voice_pipeline(
        _config: VoiceConfig,
        _auth: SharedVoiceAuth,
        mut cmd_rx: tokio::sync::mpsc::Receiver<VoiceCommand>,
        _event_tx: tokio::sync::mpsc::Sender<VoiceEvent>,
    ) {
        while let Some(cmd) = cmd_rx.recv().await {
            if matches!(cmd, VoiceCommand::Shutdown) {
                break;
            }
        }
    }
}

#[cfg(not(feature = "voice"))]
pub use stub::*;
