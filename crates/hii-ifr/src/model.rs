//! The HII identity and storage of a form set.
//!
//! A HII form set needs a GUID, a variable store and one question identifier
//! and storage location per value row. The `forms` model has none of them, so a
//! consumer supplies this descriptor to [`encode`](crate::encode) and receives
//! one from [`decode`](crate::decode).

use alloc::{string::String, vec::Vec};

use forms::{Control, FormSet, Key, Value};

/// A GUID in UEFI byte order.
pub type Guid = [u8; 16];

/// The stored width of a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    /// One byte.
    U8,
    /// Two bytes.
    U16,
    /// Four bytes.
    U32,
    /// Eight bytes.
    U64,
}

impl Width {
    /// Returns the width in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(hii_ifr::Width::U16.bytes(), 2);
    /// ```
    pub const fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 => 8,
        }
    }

    /// Returns the smallest width that holds a number.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(hii_ifr::Width::of(255), hii_ifr::Width::U8);
    /// assert_eq!(hii_ifr::Width::of(256), hii_ifr::Width::U16);
    /// ```
    pub const fn of(number: u64) -> Self {
        if number <= u8::MAX as u64 {
            Self::U8
        } else if number <= u16::MAX as u64 {
            Self::U16
        } else if number <= u32::MAX as u64 {
            Self::U32
        } else {
            Self::U64
        }
    }
}

/// Where a question value is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    /// The question has no stored value.
    None,
    /// A byte offset in a variable store.
    Offset {
        /// The store identifier.
        store: u16,
        /// The byte offset.
        offset: u16,
    },
    /// A named value in a name/value store.
    Name {
        /// The store identifier.
        store: u16,
        /// The value name.
        name: String,
    },
}

/// The IFR shape of a value question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestionKind {
    /// A checkbox.
    Bool,
    /// A number or choice, with its stored width.
    Number(Width),
    /// A text value.
    Text {
        /// The largest character count.
        maximum: usize,
    },
    /// A password.
    Password {
        /// The largest character count.
        maximum: usize,
    },
    /// An ordered list.
    OrderedList,
    /// An action.
    Action,
}

/// The HII identity and storage of one value row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionRef {
    /// The row key.
    pub row: Key,
    /// The question identifier.
    pub question: u16,
    /// The storage location.
    pub storage: Storage,
    /// The stored size in bytes.
    pub bytes: u16,
    /// The IFR shape.
    pub kind: QuestionKind,
    /// The question header flags (`EFI_IFR_FLAG_*`), for a consumer that needs
    /// the reset or reconnect requirement of a question.
    pub flags: u8,
}

/// The kind of a variable store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarStoreKind {
    /// A buffer that the driver routes.
    Buffer,
    /// An EFI variable.
    Efi {
        /// The variable attributes.
        attributes: u32,
    },
    /// Named values.
    NameValue,
}

/// A variable store declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarStore {
    /// The store identifier.
    pub id: u16,
    /// The store GUID.
    pub guid: Guid,
    /// The store name.
    pub name: String,
    /// The store size in bytes, or 0 for a name/value store.
    pub size: u16,
    /// The store kind.
    pub kind: VarStoreKind,
}

/// The form identifier of one page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormRef {
    /// The page key.
    pub page: Key,
    /// The form identifier.
    pub form: u16,
}

/// The HII identity, storage and refresh events of one form set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    /// The form set GUID.
    pub guid: Guid,
    /// The class GUIDs.
    pub class_guids: Vec<Guid>,
    /// The variable stores.
    pub varstores: Vec<VarStore>,
    /// The form identifier of each page.
    pub forms: Vec<FormRef>,
    /// The identity and storage of each value row.
    pub questions: Vec<QuestionRef>,
    /// The GUID of each refresh event key.
    pub refresh_events: Vec<(Key, Guid)>,
}

impl Export {
    /// Returns the question of one row.
    ///
    /// # Examples
    ///
    /// ```
    /// use hii_ifr::Export;
    ///
    /// let export = Export {
    ///     guid: [0; 16],
    ///     class_guids: Vec::new(),
    ///     varstores: Vec::new(),
    ///     forms: Vec::new(),
    ///     questions: Vec::new(),
    ///     refresh_events: Vec::new(),
    /// };
    /// assert!(export.question(forms::Key(1)).is_none());
    /// ```
    pub fn question(&self, row: Key) -> Option<&QuestionRef> {
        self.questions.iter().find(|question| question.row == row)
    }

    /// Returns the form identifier of one page.
    ///
    /// # Examples
    ///
    /// ```
    /// use hii_ifr::Export;
    ///
    /// let export = Export {
    ///     guid: [0; 16],
    ///     class_guids: Vec::new(),
    ///     varstores: Vec::new(),
    ///     forms: Vec::new(),
    ///     questions: Vec::new(),
    ///     refresh_events: Vec::new(),
    /// };
    /// assert!(export.form(forms::Key(1)).is_none());
    /// ```
    pub fn form(&self, page: Key) -> Option<u16> {
        self.forms
            .iter()
            .find(|form| form.page == page)
            .map(|form| form.form)
    }

    /// Returns the GUID of a refresh event key.
    ///
    /// # Examples
    ///
    /// ```
    /// use hii_ifr::Export;
    ///
    /// let export = Export {
    ///     guid: [0; 16],
    ///     class_guids: Vec::new(),
    ///     varstores: Vec::new(),
    ///     forms: Vec::new(),
    ///     questions: Vec::new(),
    ///     refresh_events: Vec::new(),
    /// };
    /// assert!(export.refresh_event(forms::Key(1)).is_none());
    /// ```
    pub fn refresh_event(&self, key: Key) -> Option<Guid> {
        self.refresh_events
            .iter()
            .find(|(event, _)| *event == key)
            .map(|(_, guid)| *guid)
    }
}

/// Builds a descriptor for a form set with one buffer store.
///
/// Every value row takes the next question identifier and the next byte offset
/// of a buffer store with identifier 1 and name `Forms`. A question is keyed
/// by the value binding of its row, so a condition that names the binding finds
/// it. A number takes its
/// natural width, a checkbox one byte and text twice its maximum character
/// count. An action row keeps no storage.
///
/// # Examples
///
/// ```
/// use forms::{Builder, Control, Key};
///
/// let mut builder = Builder::new(Key(1), "P");
/// builder.page("Main");
/// builder.row("Mode", Control::Toggle).set_binding(Key(10));
/// let set = builder.build();
/// let export = hii_ifr::descriptor(&set, [0x42; 16]);
/// assert_eq!(export.questions.len(), 1);
/// assert_eq!(export.questions[0].question, 1);
/// ```
pub fn descriptor(set: &FormSet, guid: Guid) -> Export {
    let forms = set
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| FormRef {
            page: page.key,
            form: u16::try_from(index + 1).unwrap_or(u16::MAX),
        })
        .collect();
    let mut questions = Vec::new();
    let mut offset = 0u16;
    let mut next = 1u16;
    for page in &set.pages {
        for row in &page.rows {
            if row.binding.is_none() && !matches!(row.control, Control::Action) {
                continue;
            }
            let (kind, bytes) = match &row.control {
                Control::Toggle => (QuestionKind::Bool, 1),
                Control::Choice(choices) => {
                    let maximum = choices.iter().map(|choice| choice.value).max().unwrap_or(0);
                    let width = Width::of(maximum);
                    (
                        QuestionKind::Number(width),
                        u16::try_from(width.bytes()).unwrap_or(8),
                    )
                }
                Control::Number { maximum, .. } | Control::Slider { maximum, .. } => {
                    let width = Width::of(*maximum);
                    (
                        QuestionKind::Number(width),
                        u16::try_from(width.bytes()).unwrap_or(8),
                    )
                }
                Control::TextInput {
                    maximum, secret, ..
                } => {
                    let bytes = u16::try_from(maximum.saturating_mul(2)).unwrap_or(u16::MAX);
                    if *secret {
                        (QuestionKind::Password { maximum: *maximum }, bytes)
                    } else {
                        (QuestionKind::Text { maximum: *maximum }, bytes)
                    }
                }
                Control::OrderedList { maximum, .. } => {
                    let width = Width::of(u64::from(u16::MAX));
                    (
                        QuestionKind::OrderedList,
                        u16::try_from(width.bytes() as u32 * (*maximum as u32 + 1))
                            .unwrap_or(u16::MAX),
                    )
                }
                Control::Action => {
                    questions.push(QuestionRef {
                        row: row.binding.unwrap_or(row.key),
                        question: next,
                        storage: Storage::None,
                        bytes: 0,
                        kind: QuestionKind::Action,
                        flags: 0,
                    });
                    next = next.saturating_add(1);
                    continue;
                }
                _ => continue,
            };
            questions.push(QuestionRef {
                row: row.binding.unwrap_or(row.key),
                question: next,
                storage: Storage::Offset { store: 1, offset },
                bytes,
                kind,
                flags: 0,
            });
            next = next.saturating_add(1);
            offset = offset.saturating_add(bytes);
        }
    }
    Export {
        guid,
        class_guids: Vec::new(),
        varstores: alloc::vec![VarStore {
            id: 1,
            guid,
            name: String::from("Forms"),
            size: offset,
            kind: VarStoreKind::Buffer,
        }],
        forms,
        questions,
        refresh_events: Vec::new(),
    }
}

/// Returns the value of a stored default.
pub(crate) fn default_value(value: &Value) -> u64 {
    match value {
        Value::Number(number) => *number,
        Value::Bool(flag) => u64::from(*flag),
        _ => 0,
    }
}
