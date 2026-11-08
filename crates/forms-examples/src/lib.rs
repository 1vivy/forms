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

use forms::{
    Builder, Choice, Control, Error, Expr, FormSet, Key, MemoryProvider, Provider, Request, Row,
    Scope, Value,
};

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

/// The value key of the gallery toggle row.
pub const GALLERY_TOGGLE: Key = Key(100);
/// The value key of the gallery choice row.
pub const GALLERY_CHOICE: Key = Key(101);
/// The value key of the gallery number row.
pub const GALLERY_NUMBER: Key = Key(102);
/// The value key of the gallery slider row.
pub const GALLERY_SLIDER: Key = Key(103);
/// The value key of the gallery text row.
pub const GALLERY_TEXT: Key = Key(104);
/// The value key of the gallery password row.
pub const GALLERY_PASSWORD: Key = Key(105);
/// The value key of the gallery ordered-list row.
pub const GALLERY_ORDER: Key = Key(106);
/// The value key of the toggle on the second gallery page.
pub const GALLERY_SECOND: Key = Key(107);
/// The value key of the grayed-out gallery row.
pub const GALLERY_DISABLED: Key = Key(108);
/// The value key of the gallery row with long help.
pub const GALLERY_GUARD: Key = Key(109);
/// The value key of the gallery row with a scrolling picker.
pub const GALLERY_BOOT: Key = Key(110);
/// The default set that the gallery reset row writes.
pub const GALLERY_DEFAULTS: Key = Key(140);

/// The labels of the gallery choice row.
pub const GALLERY_CHOICES: [&str; 18] = [
    "Fast boot",
    "Safe boot",
    "Recovery",
    "Network boot",
    "USB boot",
    "Disk boot",
    "Shell",
    "Diagnostics",
    "Firmware update",
    "Factory reset",
    "Sleep",
    "Shutdown",
    "IPv4 boot",
    "IPv6 boot",
    "Legacy boot",
    "Memory test",
    "Rescue shell",
    "Power off",
];

/// The labels of the gallery ordered-list row.
pub const GALLERY_ORDER_CHOICES: [&str; 4] = ["Disk", "USB", "Network", "Shell"];

/// Builds the gallery form set: one row for every built-in control kind.
///
/// The set has two pages. The first page holds the gallery, and its link row
/// opens the second page. The action row is the destination picker that a
/// front end may open as a modal.
///
/// # Examples
///
/// ```
/// let set = forms_examples::gallery();
/// assert_eq!(set.title, "Gallery");
/// assert_eq!(set.pages.len(), 2);
/// ```
pub fn gallery() -> FormSet {
    let mut builder = Builder::new(Key(2), "Gallery").help("Every control kind, in one set.");
    let second = builder.key();
    builder.page("Controls");
    builder.row("Controls", Control::Heading);
    builder.row(
        "About this page",
        Control::Text(String::from("Every built-in control kind, in one set.")),
    );
    builder
        .link("Second page", second)
        .set_help("Open a second page of the same set.");
    builder
        .row("Destination picker", Control::Action)
        .set_help("Open a modal with one button for each destination.");
    builder
        .row("Secure boot", Control::Toggle)
        .set_binding(GALLERY_TOGGLE)
        .set_help("Verify the boot loader signature.");
    builder
        .row(
            "Startup",
            Control::Choice(gallery_choices(&GALLERY_CHOICES)),
        )
        .set_binding(GALLERY_CHOICE)
        .set_help("The mode that the payload starts in.");
    builder
        .row("Boot entries", Control::Choice(boot_entries()))
        .set_binding(GALLERY_BOOT)
        .set_help("A list longer than the picker, so the picker scrolls.");
    builder
        .row(
            "Cores",
            Control::Number {
                minimum: 1,
                maximum: 64,
                step: 1,
            },
        )
        .set_binding(GALLERY_NUMBER)
        .set_help("The number of cores that the payload starts.");
    builder
        .row(
            "Menu timeout",
            Control::Slider {
                minimum: 0,
                maximum: 10,
                step: 1,
                unit: String::from("s"),
                labels: vec![Choice::new("Off", 0)],
            },
        )
        .set_binding(GALLERY_SLIDER)
        .set_help("How long the boot menu waits. Zero waits forever.");
    builder
        .row(
            "Machine name",
            Control::TextInput {
                minimum: 0,
                maximum: 24,
                secret: false,
            },
        )
        .set_binding(GALLERY_TEXT)
        .set_help("The name that the payload receives.");
    builder
        .row(
            "Password",
            Control::TextInput {
                minimum: 0,
                maximum: 24,
                secret: true,
            },
        )
        .set_binding(GALLERY_PASSWORD)
        .set_help("The secret that the boot loader passes.");
    builder
        .row(
            "Disk order",
            Control::OrderedList {
                choices: gallery_choices(&GALLERY_ORDER_CHOICES),
                maximum: 3,
            },
        )
        .set_binding(GALLERY_ORDER)
        .set_help("Choose the order in which the payload tries the disks.");
    builder
        .row("Restore gallery values", Control::Reset(GALLERY_DEFAULTS))
        .set_help("Write the named default set.");
    builder.row(
        "Progress",
        Control::Progress {
            current: 1,
            total: Some(4),
        },
    );
    builder.row("Divider", Control::Divider);
    let update = builder.row("Firmware update", Control::Toggle);
    update.set_binding(GALLERY_DISABLED);
    update.disabled = Some(Expr::Bool(true));
    update.set_help("This board has no firmware update slot. The row is grayed out.");
    builder
        .row(
            "Boot guard",
            Control::Slider {
                minimum: 0,
                maximum: 5,
                step: 1,
                unit: String::from("x"),
                labels: Vec::new(),
            },
        )
        .set_binding(GALLERY_GUARD)
        .set_help(
            "This help is long so that the help box wraps it over several lines. A phone page \
             still has to explain a value well, and this row shows how the box fills its lines \
             and marks the text that it does not show.",
        );
    builder
        .row(
            "Imported control",
            Control::Unsupported(String::from("Firmware opcode 0x1234")),
        )
        .set_help("An imported row that this crate cannot show. It stays visible.");
    builder.page_with(second, "Second page");
    builder.row("Second page", Control::Heading);
    builder.row(
        "Reached the link",
        Control::Text(String::from("The link row opened this page.")),
    );
    builder
        .row("Network boot", Control::Toggle)
        .set_binding(GALLERY_SECOND)
        .set_help("A value on the second page.");
    builder.defaults(
        GALLERY_DEFAULTS,
        "Gallery defaults",
        [
            (GALLERY_TOGGLE, Value::Bool(false)),
            (GALLERY_CHOICE, Value::Number(0)),
            (GALLERY_NUMBER, Value::Number(3)),
            (GALLERY_SLIDER, Value::Number(5)),
            (GALLERY_ORDER, Value::List(vec![1])),
            (GALLERY_SECOND, Value::Bool(false)),
            (GALLERY_GUARD, Value::Number(1)),
            (GALLERY_DISABLED, Value::Bool(false)),
            (GALLERY_BOOT, Value::Number(0)),
        ],
    );
    builder.build()
}

/// Returns the choices of the row whose picker scrolls.
fn boot_entries() -> Vec<Choice> {
    (0..60u64)
        .map(|index| Choice::new(format!("Boot entry {index:02}"), index))
        .collect()
}

/// Returns a provider over the gallery values, seeded so that every row shows
/// a stored value.
///
/// The handle is [`HANDLE`], the same handle that [`SettingsProvider`] uses
/// for its own set. A front end publishes one set at a time.
///
/// # Examples
///
/// ```
/// use forms::{Store, Value};
///
/// let mut provider = forms_examples::gallery_provider();
/// assert_eq!(
///     provider.store.read(forms_examples::GALLERY_CHOICE),
///     Ok(Value::Number(2))
/// );
/// ```
pub fn gallery_provider() -> MemoryProvider {
    let mut provider = MemoryProvider::new(vec![(HANDLE, gallery())]);
    provider.set(GALLERY_TOGGLE, Value::Bool(true));
    provider.set(GALLERY_CHOICE, Value::Number(2));
    provider.set(GALLERY_NUMBER, Value::Number(4));
    provider.set(GALLERY_SLIDER, Value::Number(5));
    provider.set(GALLERY_TEXT, Value::Text(String::from("gallery")));
    provider.set(GALLERY_PASSWORD, Value::Text(String::from("secret")));
    provider.set(GALLERY_ORDER, Value::List(vec![2, 1]));
    provider.set(GALLERY_SECOND, Value::Bool(true));
    provider.set(GALLERY_DISABLED, Value::Bool(false));
    provider.set(GALLERY_GUARD, Value::Number(3));
    provider.set(GALLERY_BOOT, Value::Number(4));
    provider
}

/// Returns the choice values of a list of labels, in order.
fn gallery_choices(labels: &[&str]) -> Vec<Choice> {
    labels
        .iter()
        .enumerate()
        .map(|(index, label)| Choice::new(*label, u64::try_from(index).unwrap_or(u64::MAX)))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::gallery;
    use forms::controls::{BUILTIN_KINDS, kind_of};

    #[test]
    fn the_gallery_holds_every_builtin_kind() {
        let set = gallery();
        for kind in BUILTIN_KINDS {
            assert!(
                set.pages
                    .iter()
                    .flat_map(|page| page.rows.iter())
                    .any(|row| kind_of(&row.control) == kind),
                "the gallery has no row of kind {kind:?}"
            );
        }
    }
}
