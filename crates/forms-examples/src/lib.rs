//! Small compiled examples of the [`forms`] engine.
//!
//! This crate is a teaching companion to [`forms`], not a library for a
//! product. The programs under `examples` show the four parts that a front end
//! has to join: a templated formset, a [`Provider`] over typed settings, a
//! modal popup, and the handling of [`ActionRequest`]. The compiler checks
//! every example on each gate, so the code that the documentation shows is the
//! code that builds.
//!
//! The lessons, in order:
//!
//! 1. A template table ([`FIELDS`] and [`ACTIONS`]) declares the formset with
//!    [`formset`], and the same table maps a question identifier to one typed
//!    field of [`Settings`]. One list is the only authority, so a new setting
//!    reaches the page and the provider together.
//! 2. [`SettingsProvider`] owns every value. The session owns navigation
//!    only, so a front end never keeps a copy of a value.
//! 3. [`Session::open_popup`](forms::Session::open_popup) shows a modal
//!    decision over the running page, and the person answers it through the
//!    page input. A front end acts on [`Event::Popup`](forms::Event::Popup).
//! 4. A provider answers a change or an action with a request. The session
//!    acts on it before it returns [`Event::Response`](forms::Event::Response),
//!    so a front end only has to obey [`ActionRequest::Reset`] itself.
//!
//! The crate is `no_std` with `alloc`, as the engine is, so the same provider
//! works in a UEFI image and in a Linux program.
//!
//! # Examples
//!
//! ```
//! let set = forms_examples::formset();
//! assert_eq!(set.forms.first().map(|form| form.id), Some(forms_examples::FORM));
//! ```

#![no_std]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};
use forms::{
    ActionRequest, Choice, Error, Form, FormId, FormSet, Guid, Provider, Question, QuestionFlags,
    QuestionId, Scope, Statement, Storage, Value, Width,
};

/// The GUID of the example formset.
pub const SET_GUID: Guid = [0x5e; 16];

/// The handle that [`SettingsProvider`] uses for its formset.
pub const HANDLE: u8 = 1;

/// The identifier of the only form of the formset.
pub const FORM: FormId = FormId(1);

/// The provider does not know the question.
pub const UNKNOWN_QUESTION: u64 = 1;

/// The value has the wrong type for the question.
pub const WRONG_TYPE: u64 = 2;

/// The value is outside the range of the question.
pub const OUT_OF_RANGE: u64 = 3;

/// One row of the value part of the template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    /// The question identifier.
    pub question: QuestionId,
    /// The prompt of the row.
    pub prompt: &'static str,
    /// The help text of the row.
    pub help: &'static str,
    /// The part of [`Settings`] that holds the value.
    pub kind: Kind,
}

/// The statement of one [`Field`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A boolean question.
    Toggle {
        /// The default value.
        default: bool,
    },
    /// A choice among listed values.
    Choice(&'static [ChoiceSpec]),
    /// A bounded number.
    Number {
        /// The stored width of the value.
        width: Width,
        /// The smallest accepted value.
        minimum: u64,
        /// The largest accepted value.
        maximum: u64,
        /// The increment.
        step: u64,
    },
    /// A bounded text value.
    Text {
        /// The smallest accepted length in characters.
        minimum: u8,
        /// The largest accepted length in characters.
        maximum: u8,
    },
}

/// One choice of a [`Kind::Choice`] field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoiceSpec {
    /// The choice text.
    pub text: &'static str,
    /// The stored value.
    pub value: u64,
    /// This choice is the default.
    pub default: bool,
}

/// One action row of the template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    /// The question identifier.
    pub question: QuestionId,
    /// The prompt of the row.
    pub prompt: &'static str,
    /// The help text of the row.
    pub help: &'static str,
    /// The request that the provider returns.
    pub request: ActionRequest,
}

/// The value part of the template: the questions that hold a setting.
pub const FIELDS: &[Field] = &[
    Field {
        question: QuestionId(1),
        prompt: "Night mode",
        help: "Use the dark palette.",
        kind: Kind::Toggle { default: false },
    },
    Field {
        question: QuestionId(2),
        prompt: "Scale",
        help: "Interface scale.",
        kind: Kind::Choice(&[
            ChoiceSpec {
                text: "100%",
                value: 100,
                default: true,
            },
            ChoiceSpec {
                text: "150%",
                value: 150,
                default: false,
            },
            ChoiceSpec {
                text: "200%",
                value: 200,
                default: false,
            },
        ]),
    },
    Field {
        question: QuestionId(3),
        prompt: "Brightness",
        help: "Backlight level.",
        kind: Kind::Number {
            width: Width::U8,
            minimum: 0,
            maximum: 100,
            step: 5,
        },
    },
    Field {
        question: QuestionId(4),
        prompt: "Device name",
        help: "Name that the device shows.",
        kind: Kind::Text {
            minimum: 1,
            maximum: 12,
        },
    },
];

/// The action part of the template: the rows that ask for a request.
///
/// Each row shows one request that a provider can return, so the page tests
/// the three requests that a settings page normally needs: reset, submit and
/// exit.
pub const ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        question: QuestionId(5),
        prompt: "Reset to defaults",
        help: "Forget the changes and restart.",
        request: ActionRequest::Reset,
    },
    ActionSpec {
        question: QuestionId(6),
        prompt: "Save and exit",
        help: "Commit every change and leave the page.",
        request: ActionRequest::Submit,
    },
    ActionSpec {
        question: QuestionId(7),
        prompt: "Exit without saving",
        help: "Discard every change and leave the page.",
        request: ActionRequest::Exit,
    },
];

/// Builds the formset from the template.
///
/// The formset has one form. Its value rows come from [`FIELDS`] in order, and
/// its action rows come from [`ACTIONS`] in order.
///
/// # Examples
///
/// ```
/// let set = forms_examples::formset();
/// let form = set.form(forms_examples::FORM).expect("the formset has its form");
/// let questions = form
///     .items
///     .iter()
///     .filter(|item| item.statement.question().is_some())
///     .count();
/// assert_eq!(questions, forms_examples::FIELDS.len() + forms_examples::ACTIONS.len());
/// ```
#[must_use]
pub fn formset() -> FormSet {
    let mut items: Vec<forms::Item> = vec![
        Statement::Subtitle {
            text: "Display".into(),
        }
        .into(),
    ];
    for field in FIELDS {
        items.push(statement(field));
    }
    items.push(
        Statement::Text {
            prompt: "Provider".into(),
            help: "Where the values live.".into(),
            value: "in memory".into(),
        }
        .into(),
    );
    items.push(
        Statement::Subtitle {
            text: "Actions".into(),
        }
        .into(),
    );
    for action in ACTIONS {
        items.push(
            Statement::Action {
                question: Question {
                    id: action.question,
                    prompt: action.prompt.into(),
                    help: action.help.into(),
                    storage: Storage::None,
                    flags: QuestionFlags::CALLBACK,
                },
            }
            .into(),
        );
    }
    FormSet {
        guid: SET_GUID,
        title: "Example".into(),
        help: "A templated formset over typed settings.".into(),
        class_guids: Vec::new(),
        varstores: Vec::new(),
        forms: vec![Form {
            id: FORM,
            title: "Example".into(),
            items,
        }],
    }
}

/// Builds the statement of one field.
fn statement(field: &Field) -> forms::Item {
    let question = Question {
        id: field.question,
        prompt: field.prompt.into(),
        help: field.help.into(),
        storage: Storage::None,
        flags: QuestionFlags::default(),
    };
    let statement = match field.kind {
        Kind::Toggle { default } => Statement::Checkbox { question, default },
        Kind::Choice(choices) => Statement::OneOf {
            question,
            width: Width::U16,
            choices: choices.iter().map(choice).collect(),
        },
        Kind::Number {
            width,
            minimum,
            maximum,
            step,
        } => Statement::Numeric {
            question,
            width,
            minimum,
            maximum,
            step,
        },
        Kind::Text { minimum, maximum } => Statement::TextInput {
            question,
            minimum,
            maximum,
        },
    };
    statement.into()
}

/// Copies one template choice into a model choice.
fn choice(spec: &ChoiceSpec) -> Choice {
    Choice {
        text: spec.text.into(),
        value: spec.value,
        default: spec.default,
    }
}

/// The typed settings behind the example formset.
///
/// The provider keeps two copies: the draft that the page edits, and the
/// committed copy that a discard restores. A product replaces this struct with
/// its own facts and keeps the same shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Whether the dark palette is active.
    pub night_mode: bool,
    /// The interface scale.
    pub scale: Scale,
    /// The backlight level, 0 to 100.
    pub brightness: u8,
    /// The device name, 1 to 12 characters.
    pub name: String,
}

impl Settings {
    /// Returns the values that the template declares as the defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// let settings = forms_examples::Settings::defaults();
    /// assert_eq!(settings.scale, forms_examples::Scale::Percent100);
    /// assert!(!settings.night_mode);
    /// ```
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            night_mode: false,
            scale: Scale::Percent100,
            brightness: 50,
            name: "opq".into(),
        }
    }

    /// Returns the value of one question.
    ///
    /// # Errors
    ///
    /// Returns [`UNKNOWN_QUESTION`] when the identifier is not a value row of
    /// [`FIELDS`].
    pub fn value(&self, id: QuestionId) -> Result<Value, Error> {
        Ok(match id.0 {
            1 => Value::Bool(self.night_mode),
            2 => Value::Number(self.scale.value()),
            3 => Value::Number(u64::from(self.brightness)),
            4 => Value::Text(self.name.clone()),
            _ => return Err(error(UNKNOWN_QUESTION, "the question holds no setting")),
        })
    }

    /// Stores the value of one question.
    ///
    /// # Errors
    ///
    /// Returns [`UNKNOWN_QUESTION`] for an action row, [`WRONG_TYPE`] for a
    /// value of the wrong type, and [`OUT_OF_RANGE`] for a value that the
    /// template does not offer.
    pub fn set(&mut self, id: QuestionId, value: &Value) -> Result<(), Error> {
        match (id.0, value) {
            (1, Value::Bool(on)) => self.night_mode = *on,
            (2, Value::Number(number)) => {
                let scale = Scale::from_value(*number)
                    .ok_or_else(|| error(OUT_OF_RANGE, "the scale is not one of the choices"))?;
                self.scale = scale;
            }
            (3, Value::Number(number)) => {
                let level = u8::try_from(*number)
                    .ok()
                    .filter(|level| *level <= 100)
                    .ok_or_else(|| error(OUT_OF_RANGE, "the brightness is above 100"))?;
                self.brightness = level;
            }
            (4, Value::Text(text)) => {
                if text.is_empty() || text.chars().count() > 12 {
                    return Err(error(OUT_OF_RANGE, "the name is 1 to 12 characters"));
                }
                self.name.clone_from(text);
            }
            (1..=4, _) => return Err(error(WRONG_TYPE, "the value has the wrong type")),
            _ => return Err(error(UNKNOWN_QUESTION, "the question holds no setting")),
        }
        Ok(())
    }

    /// Copies one question value from another settings record.
    ///
    /// The provider uses this method for [`Scope::Question`]. A scope with more
    /// rows copies each row through it.
    ///
    /// # Errors
    ///
    /// Returns the same failure as [`Settings::value`] and [`Settings::set`].
    pub fn copy_field(&mut self, from: &Self, id: QuestionId) -> Result<(), Error> {
        let value = from.value(id)?;
        self.set(id, &value)
    }
}

/// The interface scale that the choice row offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Scale {
    /// 100 percent.
    Percent100 = 100,
    /// 150 percent.
    Percent150 = 150,
    /// 200 percent.
    Percent200 = 200,
}

impl Scale {
    /// Returns the stored number of this scale.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms_examples::Scale::Percent150.value(), 150);
    /// ```
    #[must_use]
    pub const fn value(self) -> u64 {
        self as u64
    }

    /// Returns the scale of a stored number.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms_examples::Scale;
    ///
    /// assert_eq!(Scale::from_value(200), Some(Scale::Percent200));
    /// assert_eq!(Scale::from_value(120), None);
    /// ```
    #[must_use]
    pub const fn from_value(value: u64) -> Option<Self> {
        match value {
            100 => Some(Self::Percent100),
            150 => Some(Self::Percent150),
            200 => Some(Self::Percent200),
            _ => None,
        }
    }
}

/// A provider over [`Settings`].
///
/// The provider owns the draft and the committed copy. It records every
/// request that it returns, so a front end can log or grade the exchange
/// without a second authority.
///
/// # Examples
///
/// ```
/// use forms::Provider;
/// use forms_examples::SettingsProvider;
///
/// let mut provider = SettingsProvider::new();
/// let sets = provider.formsets().expect("the list is readable");
/// assert_eq!(sets.len(), 1);
/// ```
pub struct SettingsProvider {
    draft: Settings,
    committed: Settings,
    requests: Vec<ActionRequest>,
}

impl SettingsProvider {
    /// Creates a provider over the default settings.
    ///
    /// # Examples
    ///
    /// ```
    /// let provider = forms_examples::SettingsProvider::new();
    /// assert_eq!(provider.draft(), provider.committed());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_settings(Settings::defaults())
    }

    /// Creates a provider over `settings`.
    ///
    /// The draft and the committed copy start with the same values.
    #[must_use]
    pub fn with_settings(settings: Settings) -> Self {
        Self {
            draft: settings.clone(),
            committed: settings,
            requests: Vec::new(),
        }
    }

    /// Returns the values as the page sees them.
    #[must_use]
    pub fn draft(&self) -> &Settings {
        &self.draft
    }

    /// Returns the values of the last commit.
    #[must_use]
    pub fn committed(&self) -> &Settings {
        &self.committed
    }

    /// Returns every request that the provider returned, in order.
    #[must_use]
    pub fn requests(&self) -> &[ActionRequest] {
        &self.requests
    }

    /// Records one returned request and passes it back.
    fn note(&mut self, request: ActionRequest) -> ActionRequest {
        self.requests.push(request);
        request
    }

    /// Copies the values of one scope between the two records.
    fn copy(&mut self, scope: Scope, forward: bool) -> Result<(), Error> {
        for field in FIELDS {
            if !in_scope(field.question, scope) {
                continue;
            }
            if forward {
                self.committed.copy_field(&self.draft, field.question)?;
            } else {
                self.draft.copy_field(&self.committed, field.question)?;
            }
        }
        Ok(())
    }
}

impl Default for SettingsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for SettingsProvider {
    type Handle = u8;

    fn formsets(&mut self) -> Result<Vec<(u8, FormSet)>, Error> {
        Ok(vec![(HANDLE, formset())])
    }

    fn read(&mut self, _handle: u8, _set: &FormSet, question: &Question) -> Result<Value, Error> {
        if is_action(question.id) {
            return Ok(Value::Number(0));
        }
        self.draft.value(question.id)
    }

    fn write(
        &mut self,
        _handle: u8,
        _set: &FormSet,
        question: &Question,
        value: &Value,
    ) -> Result<ActionRequest, Error> {
        self.draft.set(question.id, value)?;
        Ok(self.note(ActionRequest::None))
    }

    fn action(
        &mut self,
        _handle: u8,
        _set: &FormSet,
        question: &Question,
    ) -> Result<ActionRequest, Error> {
        let action = ACTIONS
            .iter()
            .find(|action| action.question == question.id)
            .ok_or_else(|| error(UNKNOWN_QUESTION, "the question is not an action"))?;
        Ok(self.note(action.request))
    }

    fn apply(&mut self, _handle: u8, _set: &FormSet, scope: Scope) -> Result<(), Error> {
        self.copy(scope, true)
    }

    fn discard(&mut self, _handle: u8, _set: &FormSet, scope: Scope) -> Result<(), Error> {
        self.copy(scope, false)
    }
}

/// Returns whether the identifier is an action row.
fn is_action(id: QuestionId) -> bool {
    ACTIONS.iter().any(|action| action.question == id)
}

/// Returns whether a question lies in a scope.
fn in_scope(id: QuestionId, scope: Scope) -> bool {
    match scope {
        Scope::FormSet => true,
        Scope::Form(form) => form == FORM,
        Scope::Question(question) => question == id,
    }
}

/// Builds one provider failure.
fn error(code: u64, reason: &str) -> Error {
    Error {
        code,
        reason: reason.to_string(),
    }
}
