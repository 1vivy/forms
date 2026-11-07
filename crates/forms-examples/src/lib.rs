//! Small compiled examples of the [`forms`] crate.
//!
//! This crate is a teaching companion, not a product library. It shows the
//! four parts that a front end joins: a form set built from one template
//! table, a provider over typed settings, a modal decision, and the handling
//! of a [`Request`]. The compiler checks every example on each gate.
//!
//! The lessons:
//!
//! 1. One table ([`FIELDS`]) declares the page and the mapping from a value
//!    key to one typed field of [`Settings`], so a new setting reaches both
//!    together.
//! 2. [`SettingsProvider`] owns every value. The session owns navigation only.
//! 3. [`forms::Session::open_modal`] shows a decision over the running page,
//!    and the front end acts on [`forms::Event::Modal`].
//! 4. A provider returns a [`Request`]. The session acts on it before it
//!    returns [`forms::Event::Response`], so a front end only obeys a restart.
//!
//! # Examples
//!
//! ```
//! let set = forms_examples::formset();
//! assert_eq!(set.pages[0].rows.len(), 10);
//! ```
#![no_std]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use forms::{Builder, Choice, Control, Error, FormSet, Key, Provider, Request, Row, Scope, Value};

/// The handle that [`SettingsProvider`] uses for its form set.
pub const HANDLE: u8 = 1;

/// The provider does not know the field.
pub const UNKNOWN_FIELD: u64 = 1;

/// The value has the wrong type for the field.
pub const WRONG_TYPE: u64 = 2;

/// The value is outside the range of the field.
pub const OUT_OF_RANGE: u64 = 3;

/// The value key of the secure boot field.
pub const SECURE_BOOT: Key = Key(10);
/// The value key of the mode field.
pub const MODE: Key = Key(11);
/// The value key of the core count field.
pub const CORES: Key = Key(12);
/// The value key of the name field.
pub const NAME: Key = Key(13);
/// The value key of the timeout field.
pub const TIMEOUT: Key = Key(14);
/// The default set that the restore row writes.
pub const FACTORY: Key = Key(40);

/// The typed setting behind one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    /// Whether secure boot is on.
    SecureBoot,
    /// The boot mode.
    Mode,
    /// The number of cores.
    Cores,
    /// The machine name.
    Name,
    /// How long the boot menu waits, in seconds.
    Timeout,
    /// The command that applies the settings.
    Apply,
}

impl Setting {
    /// Returns the value key of the setting.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms_examples::Setting::Timeout.key(), forms_examples::TIMEOUT);
    /// ```
    pub const fn key(self) -> Key {
        match self {
            Self::SecureBoot => SECURE_BOOT,
            Self::Mode => MODE,
            Self::Cores => CORES,
            Self::Name => NAME,
            Self::Timeout => TIMEOUT,
            Self::Apply => Key(20),
        }
    }
}

/// The typed settings of the example.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Whether secure boot is on.
    pub secure_boot: bool,
    /// The boot mode: 1 for fast, 2 for safe.
    pub mode: u64,
    /// The number of cores.
    pub cores: u64,
    /// The machine name.
    pub name: String,
    /// How long the boot menu waits, in seconds.
    pub timeout: u64,
    /// How often the apply command ran.
    pub applied: u32,
}

/// The control of one [`FieldSpec`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlSpec {
    /// A logical switch.
    Toggle,
    /// A choice between two modes.
    Mode,
    /// A bounded number.
    Cores,
    /// A bounded text.
    Name,
    /// A bounded number that two keys adjust.
    Timeout,
    /// A command.
    Action,
}

/// One row of the template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// The typed field.
    pub setting: Setting,
    /// The label of the row.
    pub label: &'static str,
    /// The help text of the row.
    pub help: &'static str,
    /// The control of the row.
    pub control: ControlSpec,
}

/// Every row of the example page.
///
/// This table is the only authority for the page and for the provider.
pub const FIELDS: [FieldSpec; 6] = [
    FieldSpec {
        setting: Setting::SecureBoot,
        label: "Secure boot",
        help: "Verify the boot loader signature.",
        control: ControlSpec::Toggle,
    },
    FieldSpec {
        setting: Setting::Mode,
        label: "Boot mode",
        help: "Trade boot time for recovery options.",
        control: ControlSpec::Mode,
    },
    FieldSpec {
        setting: Setting::Cores,
        label: "Cores",
        help: "The number of cores that the payload starts.",
        control: ControlSpec::Cores,
    },
    FieldSpec {
        setting: Setting::Name,
        label: "Machine name",
        help: "The name that the payload receives.",
        control: ControlSpec::Name,
    },
    FieldSpec {
        setting: Setting::Timeout,
        label: "Menu timeout",
        help: "How long the boot menu waits. Zero waits forever.",
        control: ControlSpec::Timeout,
    },
    FieldSpec {
        setting: Setting::Apply,
        label: "Apply settings",
        help: "Write the changed values.",
        control: ControlSpec::Action,
    },
];

/// Returns the setting of a value key.
///
/// # Examples
///
/// ```
/// assert_eq!(
///     forms_examples::setting_of(forms_examples::NAME),
///     Some(forms_examples::Setting::Name)
/// );
/// assert!(forms_examples::setting_of(forms::Key(99)).is_none());
/// ```
pub fn setting_of(key: Key) -> Option<Setting> {
    FIELDS
        .iter()
        .map(|field| field.setting)
        .find(|setting| setting.key() == key)
}

/// Builds the example form set from [`FIELDS`].
///
/// # Examples
///
/// ```
/// let set = forms_examples::formset();
/// assert_eq!(set.title, "Platform");
/// assert_eq!(set.pages[0].rows.len(), 10);
/// ```
pub fn formset() -> FormSet {
    let mut builder = Builder::new(Key(1), "Platform").help("Recovery settings");
    builder.page("Boot");
    builder.row("Settings", Control::Heading);
    for field in FIELDS {
        let control = match field.control {
            ControlSpec::Toggle => Control::Toggle,
            ControlSpec::Mode => {
                Control::Choice(vec![Choice::new("Fast", 1), Choice::new("Safe", 2)])
            }
            ControlSpec::Cores => Control::Number {
                minimum: 1,
                maximum: 64,
                step: 1,
            },
            ControlSpec::Name => Control::TextInput {
                minimum: 1,
                maximum: 16,
                secret: false,
            },
            ControlSpec::Timeout => Control::Slider {
                minimum: 0,
                maximum: 10,
                step: 1,
                unit: String::from("s"),
                labels: vec![Choice::new("Off", 0)],
            },
            ControlSpec::Action => Control::Action,
        };
        let row = builder.row(field.label, control);
        row.set_help(field.help);
        if field.control != ControlSpec::Action {
            row.set_binding(field.setting.key());
        }
    }
    builder.row("Restore defaults", Control::Reset(FACTORY));
    builder.row(
        "Progress",
        Control::Progress {
            current: 1,
            total: Some(4),
        },
    );
    builder.row("Separator", Control::Divider);
    builder.defaults(
        FACTORY,
        "Factory",
        [
            (SECURE_BOOT, Value::Bool(false)),
            (MODE, Value::Number(2)),
            (CORES, Value::Number(4)),
            (TIMEOUT, Value::Number(5)),
        ],
    );
    builder.build()
}

/// Reads one value of the typed settings.
///
/// # Errors
///
/// Returns [`UNKNOWN_FIELD`] for a key that no field uses.
///
/// # Examples
///
/// ```
/// use forms::{Key, Value};
///
/// let settings = forms_examples::Settings::default();
/// assert_eq!(
///     forms_examples::value_of(&settings, forms_examples::Setting::Mode),
///     Value::Number(0)
/// );
/// ```
pub fn value_of(settings: &Settings, setting: Setting) -> Value {
    match setting {
        Setting::SecureBoot => Value::Bool(settings.secure_boot),
        Setting::Mode => Value::Number(settings.mode),
        Setting::Cores => Value::Number(settings.cores),
        Setting::Name => Value::Text(settings.name.clone()),
        Setting::Timeout => Value::Number(settings.timeout),
        Setting::Apply => Value::Bytes(Vec::new()),
    }
}

/// Writes one value into the typed settings.
///
/// # Errors
///
/// Returns [`WRONG_TYPE`] for a value of another type and [`OUT_OF_RANGE`]
/// for a value outside the range of its control.
///
/// # Examples
///
/// ```
/// use forms::{Key, Value};
///
/// let mut settings = forms_examples::Settings::default();
/// forms_examples::write_setting(&mut settings, forms::Key(11), &Value::Number(1)).unwrap();
/// assert_eq!(settings.mode, 1);
/// ```
pub fn write_setting(settings: &mut Settings, key: Key, value: &Value) -> Result<(), Error> {
    let Some(setting) = setting_of(key) else {
        return Err(Error {
            code: UNKNOWN_FIELD,
            reason: String::from("No field has this key"),
        });
    };
    let number = |value: &Value| match value {
        Value::Number(number) => Ok(*number),
        _ => Err(Error {
            code: WRONG_TYPE,
            reason: String::from("The field takes a number"),
        }),
    };
    match setting {
        Setting::SecureBoot => match value {
            Value::Bool(flag) => settings.secure_boot = *flag,
            _ => {
                return Err(Error {
                    code: WRONG_TYPE,
                    reason: String::from("The field takes a logical value"),
                });
            }
        },
        Setting::Mode => {
            let mode = number(value)?;
            if !(1..=2).contains(&mode) {
                return Err(out_of_range("Mode", 1, 2));
            }
            settings.mode = mode;
        }
        Setting::Cores => {
            let cores = number(value)?;
            if !(1..=64).contains(&cores) {
                return Err(out_of_range("Cores", 1, 64));
            }
            settings.cores = cores;
        }
        Setting::Name => match value {
            Value::Text(text) => {
                if text.chars().count() > 16 {
                    return Err(out_of_range("Machine name", 0, 16));
                }
                settings.name = text.clone();
            }
            _ => {
                return Err(Error {
                    code: WRONG_TYPE,
                    reason: String::from("The field takes text"),
                });
            }
        },
        Setting::Timeout => {
            let timeout = number(value)?;
            if timeout > 10 {
                return Err(out_of_range("Menu timeout", 0, 10));
            }
            settings.timeout = timeout;
        }
        Setting::Apply => {
            settings.applied = settings.applied.saturating_add(1);
        }
    }
    Ok(())
}

/// Builds a range error.
fn out_of_range(label: &str, minimum: u64, maximum: u64) -> Error {
    Error {
        code: OUT_OF_RANGE,
        reason: format!("{label} accepts {minimum} to {maximum}"),
    }
}

/// A provider over typed [`Settings`].
///
/// # Examples
///
/// ```
/// use forms::{Provider, Value};
///
/// let mut provider = forms_examples::SettingsProvider::new();
/// let set = forms_examples::formset();
/// let row = &set.pages[0].rows[5];
/// assert_eq!(
///     provider.read(forms_examples::HANDLE, &set, row),
///     Ok(Value::Number(5))
/// );
/// ```
#[derive(Clone, Debug)]
pub struct SettingsProvider {
    settings: Settings,
    actions: Vec<Key>,
}

impl Default for SettingsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsProvider {
    /// Creates a provider with the factory settings.
    ///
    /// # Examples
    ///
    /// ```
    /// let provider = forms_examples::SettingsProvider::new();
    /// assert_eq!(provider.settings().timeout, 5);
    /// ```
    pub fn new() -> Self {
        Self {
            settings: Settings {
                secure_boot: false,
                mode: 2,
                cores: 4,
                name: String::from("board"),
                timeout: 5,
                applied: 0,
            },
            actions: Vec::new(),
        }
    }

    /// Returns the typed settings.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms_examples::SettingsProvider::new().settings().cores, 4);
    /// ```
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Returns the current value of one value key.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::Value;
    ///
    /// let provider = forms_examples::SettingsProvider::new();
    /// assert_eq!(
    ///     provider.read_setting(forms_examples::CORES),
    ///     Value::Number(4)
    /// );
    /// ```
    pub fn read_setting(&self, key: Key) -> Value {
        setting_of(key).map_or_else(
            || Value::Bytes(Vec::new()),
            |setting| value_of(&self.settings, setting),
        )
    }

    /// Returns the action keys that ran, in order.
    ///
    /// # Examples
    ///
    /// ```
    /// assert!(forms_examples::SettingsProvider::new().actions().is_empty());
    /// ```
    pub fn actions(&self) -> &[Key] {
        &self.actions
    }
}

impl Provider for SettingsProvider {
    type Handle = u8;

    fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error> {
        Ok(vec![(HANDLE, formset())])
    }

    fn read(&mut self, _handle: Self::Handle, _set: &FormSet, row: &Row) -> Result<Value, Error> {
        let key = row.binding.ok_or_else(|| Error {
            code: UNKNOWN_FIELD,
            reason: String::from("The row has no value"),
        })?;
        match setting_of(key) {
            Some(setting) => Ok(value_of(&self.settings, setting)),
            None => Err(Error {
                code: UNKNOWN_FIELD,
                reason: String::from("No field has this key"),
            }),
        }
    }

    fn write(
        &mut self,
        _handle: Self::Handle,
        _set: &FormSet,
        row: &Row,
        value: &Value,
    ) -> Result<Request, Error> {
        let key = row.binding.ok_or_else(|| Error {
            code: UNKNOWN_FIELD,
            reason: String::from("The row has no value"),
        })?;
        write_setting(&mut self.settings, key, value)?;
        Ok(Request::None)
    }

    fn action(
        &mut self,
        _handle: Self::Handle,
        set: &FormSet,
        row: &Row,
    ) -> Result<Request, Error> {
        let _ = set;
        self.actions.push(row.key);
        self.settings.applied = self.settings.applied.saturating_add(1);
        Ok(Request::Apply {
            scope: Scope::Set,
            leave: false,
        })
    }

    fn apply(&mut self, _handle: Self::Handle, _set: &FormSet, _scope: Scope) -> Result<(), Error> {
        Ok(())
    }

    fn discard(
        &mut self,
        _handle: Self::Handle,
        _set: &FormSet,
        _scope: Scope,
    ) -> Result<(), Error> {
        Ok(())
    }
}

impl SettingsProvider {
    /// Sets one typed field, for a test or a script.
    ///
    /// # Errors
    ///
    /// Returns the write failure of the field.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::Value;
    ///
    /// let mut provider = forms_examples::SettingsProvider::new();
    /// provider.set(forms_examples::CORES, Value::Number(8)).unwrap();
    /// assert_eq!(provider.settings().cores, 8);
    /// ```
    pub fn set(&mut self, key: Key, value: Value) -> Result<(), Error> {
        write_setting(&mut self.settings, key, &value)
    }
}

impl Settings {
    /// Returns the name for a log line.
    ///
    /// # Examples
    ///
    /// ```
    /// assert!(forms_examples::Settings::default().summary().contains("cores"));
    /// ```
    pub fn summary(&self) -> String {
        format!(
            "secure_boot={} mode={} cores={} name={} timeout={} applied={}",
            self.secure_boot, self.mode, self.cores, self.name, self.timeout, self.applied
        )
    }
}

/// Returns the choice labels of the mode control.
///
/// # Examples
///
/// ```
/// assert_eq!(forms_examples::modes().len(), 2);
/// ```
pub fn modes() -> &'static [&'static str] {
    &["Fast", "Safe"]
}

/// The label of the row that restores the factory values.
pub const RESTORE: &str = "Restore defaults";

/// Returns the row label of a setting.
///
/// # Examples
///
/// ```
/// assert_eq!(
///     forms_examples::label_of(forms_examples::Setting::Cores),
///     "Cores"
/// );
/// ```
pub fn label_of(setting: Setting) -> &'static str {
    FIELDS
        .iter()
        .find(|field| field.setting == setting)
        .map_or("", |field| field.label)
}

/// Returns the description of one setting for a log line.
///
/// # Examples
///
/// ```
/// assert!(forms_examples::describe(forms_examples::Setting::Timeout).contains("Menu"));
/// ```
pub fn describe(setting: Setting) -> String {
    let label = label_of(setting);
    let key = setting.key();
    format!("{label} (key {})", key.0)
}

/// Returns the choices of the mode control as a vector, for a consumer.
///
/// # Examples
///
/// ```
/// assert_eq!(forms_examples::mode_choices()[0].label, "Fast");
/// ```
pub fn mode_choices() -> Vec<Choice> {
    modes()
        .iter()
        .enumerate()
        .map(|(index, label)| Choice::new(*label, index as u64 + 1))
        .collect()
}

/// Returns the control of a setting.
///
/// # Examples
///
/// ```
/// assert_eq!(
///     forms_examples::control_of(forms_examples::Setting::SecureBoot),
///     forms_examples::ControlSpec::Toggle
/// );
/// ```
pub fn control_of(setting: Setting) -> ControlSpec {
    FIELDS
        .iter()
        .find(|field| field.setting == setting)
        .map_or(ControlSpec::Toggle, |field| field.control)
}

/// Re-exported error type name for a consumer.
pub use forms::Error as ProviderError;

/// Returns the label of an error for a front end.
///
/// # Examples
///
/// ```
/// let error = forms::Error::new("Bad value");
/// assert_eq!(forms_examples::error_label(&error), "Bad value");
/// ```
pub fn error_label(error: &Error) -> &str {
    &error.reason
}

/// Returns the text of a value for a report.
///
/// # Examples
///
/// ```
/// assert_eq!(forms_examples::value_label(&forms::Value::Bool(true)), "true");
/// ```
pub fn value_label(value: &Value) -> String {
    match value {
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Text(text) => text.clone(),
        Value::List(values) => format!("{values:?}"),
        Value::Bytes(bytes) => format!("{} bytes", bytes.len()),
    }
}
