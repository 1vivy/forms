//! Provider-backed navigation and editing.

use crate::{
    ActionRequest, Error, Expr, FormId, FormSet, Guid, Popup, PopupSelection, Provider,
    QuestionFlags, QuestionId, Scope, Statement, Value,
};
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

/// One front-end input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Select the previous row or choice.
    Up,
    /// Select the next row or choice.
    Down,
    /// Decrease a number or choice.
    Left,
    /// Increase a number or choice.
    Right,
    /// Open or accept the selection.
    Enter,
    /// Cancel an editor or leave a page.
    Back,
    /// Select the first available row.
    Home,
    /// Remove the last character.
    Erase,
    /// Append a character.
    Char(char),
}

/// The result of one input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// No state changed.
    None,
    /// The local navigation or editor changed.
    Changed,
    /// The session acted on a provider request.
    Response(ActionRequest),
    /// A provider or validation operation failed.
    Failed(Error),
    /// The user left the root page.
    Exit,
    /// The person answered a popup.
    Popup(PopupSelection),
}

/// One visible row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The row label.
    pub prompt: String,
    /// The formatted current value.
    pub value: String,
    /// The presentation category.
    pub kind: RowKind,
    /// Whether the row can be selected.
    pub enabled: bool,
    /// Whether a condition contains unsupported expressions.
    pub uncertain: bool,
}

/// A row's presentation category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// A published formset.
    FormSet,
    /// A heading.
    Heading,
    /// Read-only information.
    Info,
    /// A link to another form.
    Link,
    /// A provider action.
    Action,
    /// A boolean question.
    Toggle,
    /// A choice question.
    Choice,
    /// A numeric question.
    Number,
    /// A text question.
    Text,
    /// The row that returns to the previous page, for a front end without a Back key.
    Back,
}

/// An open value editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editor {
    /// The question label.
    pub prompt: String,
    /// The editable state.
    pub kind: EditorKind,
}

/// The editable state of one question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorKind {
    /// A list of choice labels and the selected index.
    Choice {
        /// Choice labels in publication order.
        choices: Vec<String>,
        /// The pending choice index; `choices.len()` selects Back (cancel).
        selected: usize,
    },
    /// A decimal number and its inclusive bounds.
    Number {
        /// The entered decimal text.
        text: String,
        /// The lower bound.
        minimum: u64,
        /// The upper bound.
        maximum: u64,
    },
    /// A text value with a character limit.
    Text {
        /// The entered text.
        text: String,
        /// The maximum character count.
        maximum: u8,
    },
}

/// A renderer-independent snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// Ancestor page titles, starting at the root.
    pub path: Vec<String>,
    /// The current page title.
    pub title: String,
    /// Visible rows in display order.
    pub rows: Vec<Row>,
    /// The selected index in `rows`, if any.
    pub selected: Option<usize>,
    /// Help for the selected row.
    pub help: String,
    /// The open editor, if any.
    pub editor: Option<Editor>,
    /// A provider or validation message.
    pub message: Option<String>,
    /// The open popup, if any.
    pub popup: Option<Popup>,
}

#[derive(Clone)]
struct Page<H> {
    handle: H,
    guid: Guid,
    form: FormId,
    selected: Option<usize>,
}

struct Entry {
    source: usize,
    row: Row,
    help: String,
}

/// The `source` of the Back row; no form item has this index.
const BACK_ROW: usize = usize::MAX;

/// The prompt of the Back row. Plain ASCII, so a bitmap fallback font draws it.
const BACK_PROMPT: &str = "< Back";

/// Navigation and editing state. Values remain owned by the provider.
///
/// # Examples
///
/// ```
/// let _session = forms::Session::<u32>::new(Vec::new());
/// ```
pub struct Session<H> {
    sets: Vec<(H, FormSet)>,
    pages: Vec<Page<H>>,
    root: Option<usize>,
    editor: Option<(usize, Editor)>,
    message: Option<String>,
    /// Pages at the bottom of the stack that Back does not pop (see [`Session::enter`]).
    base: usize,
    /// Every page except the formset list ends with a Back row.
    back_row: bool,
    entry_back_row: bool,
    /// Read-only rows can be selected, so a long information page scrolls.
    readable_rows: bool,
    /// The modal popup over the current page, if any.
    popup: Option<Popup>,
}

impl<H: Copy + Eq> Session<H> {
    /// Creates a root page, sorted by formset title.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new());
    /// ```
    pub fn new(mut sets: Vec<(H, FormSet)>) -> Self {
        sets.sort_by(|a, b| a.1.title.cmp(&b.1.title));
        Self {
            sets,
            pages: Vec::new(),
            root: None,
            editor: None,
            message: None,
            base: 0,
            back_row: true,
            entry_back_row: true,
            readable_rows: false,
            popup: None,
        }
    }

    /// Chooses whether pages end with a Back row. The default is `true`.
    ///
    /// A front end with three keys (for example a phone with volume and power
    /// keys) has no Back key, so it reaches the previous page through this row.
    /// A front end with an Esc key may turn the row off.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new()).with_back_row(false);
    /// ```
    pub fn with_back_row(mut self, back_row: bool) -> Self {
        self.back_row = back_row;
        self
    }

    /// Chooses whether the pinned entry form has a Back row. The default is `true`.
    ///
    /// This setting applies only to the entry form opened by [`Session::enter`].
    /// Other pages use [`Session::with_back_row`].
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new()).with_entry_back_row(false);
    /// ```
    pub fn with_entry_back_row(mut self, entry_back_row: bool) -> Self {
        self.entry_back_row = entry_back_row;
        self
    }

    /// Chooses whether read-only rows can be selected. The default is `false`.
    ///
    /// A page of read-only information scrolls only as far as the selection
    /// moves. With this setting, Up and Down also stop on `RowKind::Info` rows,
    /// the help pane shows their help, and Enter on them changes nothing.
    /// Headings stay unselectable.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new()).with_readable_rows(true);
    /// ```
    pub fn with_readable_rows(mut self, readable_rows: bool) -> Self {
        self.readable_rows = readable_rows;
        self
    }

    /// Shows a popup over the current page.
    ///
    /// The popup takes every input until the person selects one of its legal
    /// answers. The page, its selection and its open editor stay unchanged
    /// underneath it. A front end raises a popup this way when it needs a
    /// decision, without a second session and without a firmware protocol.
    /// A second call replaces an open popup.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Input, Popup, PopupSelection, PopupStyle, PopupType, Session};
    ///
    /// let mut session = Session::<u32>::new(Vec::new());
    /// // A warning offers No first, so one Enter refuses the question.
    /// session.open_popup(Popup::new(PopupStyle::Warning, PopupType::YesNo, "Erase?"));
    /// struct Memory;
    /// impl forms::Provider for Memory {
    ///     type Handle = u32;
    ///     fn formsets(&mut self) -> Result<Vec<(u32, forms::FormSet)>, forms::Error> {
    ///         Ok(Vec::new())
    ///     }
    ///     fn read(
    ///         &mut self,
    ///         _handle: u32,
    ///         _set: &forms::FormSet,
    ///         _question: &forms::Question,
    ///     ) -> Result<forms::Value, forms::Error> {
    ///         Err(forms::Error { code: 1, reason: "no value".into() })
    ///     }
    ///     fn write(
    ///         &mut self,
    ///         _handle: u32,
    ///         _set: &forms::FormSet,
    ///         _question: &forms::Question,
    ///         _value: &forms::Value,
    ///     ) -> Result<forms::ActionRequest, forms::Error> {
    ///         Ok(forms::ActionRequest::None)
    ///     }
    ///     fn action(
    ///         &mut self,
    ///         _handle: u32,
    ///         _set: &forms::FormSet,
    ///         _question: &forms::Question,
    ///     ) -> Result<forms::ActionRequest, forms::Error> {
    ///         Ok(forms::ActionRequest::None)
    ///     }
    /// }
    /// let mut provider = Memory;
    /// assert_eq!(
    ///     session.input(Input::Enter, &mut provider),
    ///     forms::Event::Popup(PopupSelection::No)
    /// );
    /// ```
    pub fn open_popup(&mut self, popup: Popup) {
        self.popup = Some(popup);
    }

    /// Opens the entry form of the formset with this GUID and pins it.
    ///
    /// Back on the pinned form returns [`Event::Exit`] instead of the formset
    /// list, as `FormBrowser2.SendForm` does for one formset. Returns `false`
    /// and changes nothing when no formset with forms has this GUID.
    ///
    /// Several providers can publish a formset with the same GUID. The method
    /// then selects the first published formset with that GUID. Every later
    /// page, edit, write and action of the session uses the handle of that
    /// formset, never another formset with the same GUID.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut session = forms::Session::<u32>::new(Vec::new());
    /// assert!(!session.enter(&[0x11; 16]));
    /// ```
    pub fn enter(&mut self, guid: &Guid) -> bool {
        let Some((handle, form)) = self
            .sets
            .iter()
            .find(|(_, set)| set.guid == *guid)
            .and_then(|(handle, set)| set.forms.first().map(|form| (*handle, form.id)))
        else {
            return false;
        };
        self.pages = alloc::vec![Page {
            handle,
            guid: *guid,
            form,
            selected: None,
        }];
        self.base = 1;
        self.editor = None;
        self.message = None;
        true
    }

    /// Replaces published forms. Keeps surviving pages and their selection.
    ///
    /// Pages and the root selection retain both the provider handle and GUID.
    ///
    /// An open editor is cancelled because its question may have changed.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut session = forms::Session::<u32>::new(Vec::new());
    /// session.replace(Vec::new());
    /// ```
    pub fn replace(&mut self, mut sets: Vec<(H, FormSet)>) {
        let root_identity = self
            .root
            .and_then(|i| self.sets.get(i))
            .map(|s| (s.0, s.1.guid));
        sets.sort_by(|a, b| a.1.title.cmp(&b.1.title));
        self.root = root_identity
            .and_then(|(handle, guid)| sets.iter().position(|s| s.0 == handle && s.1.guid == guid));
        let valid = self
            .pages
            .iter()
            .take_while(|page| {
                sets.iter().any(|(handle, set)| {
                    *handle == page.handle && set.guid == page.guid && set.form(page.form).is_some()
                })
            })
            .count();
        self.pages.truncate(valid);
        self.base = self.base.min(self.pages.len());
        self.sets = sets;
        self.editor = None;
    }

    /// Reads a fresh snapshot and repairs the selection.
    ///
    /// Values come from `provider`. A failed read shows `<unavailable>` and
    /// does not abort the snapshot. An open editor is returned unchanged.
    pub fn view<P: Provider<Handle = H>>(&mut self, provider: &mut P) -> View {
        let entries = self.entries(provider);
        self.repair(&entries);
        let selected = self
            .selection()
            .and_then(|source| entries.iter().position(|e| e.source == source));
        let help = selected
            .map(|i| entries[i].help.clone())
            .unwrap_or_default();
        let mut path = Vec::new();
        let mut title = String::from("Formsets");
        for (index, page) in self.pages.iter().enumerate() {
            if index >= self.base {
                path.push(title);
            }
            title = self
                .sets
                .iter()
                .find(|s| s.0 == page.handle && s.1.guid == page.guid)
                .and_then(|s| s.1.form(page.form))
                .map(|f| f.title.clone())
                .unwrap_or_default();
        }
        View {
            path,
            title,
            rows: entries.into_iter().map(|e| e.row).collect(),
            selected,
            help,
            editor: self.editor.as_ref().map(|e| e.1.clone()),
            message: self.message.clone(),
            popup: self.popup.clone(),
        }
    }

    /// Applies one input and returns navigation or provider effects.
    ///
    /// With an editor open, invalid values keep the editor open, set the
    /// message and return [`Event::Failed`] without calling the provider.
    /// Outside an editor, Back pops the form stack and returns
    /// [`Event::Exit`] at the root.
    ///
    /// Up and Down wrap between the first and the last selectable row. In a
    /// choice editor they wrap the same way, and the last entry is the Back
    /// row, which cancels the editor without a write.
    ///
    /// A provider answers a write or an action with an [`ActionRequest`]. The
    /// session commits or discards the named scope through
    /// [`Provider::apply`] or [`Provider::discard`], reconnects the controller
    /// through [`Provider::reconnect`], and leaves the form or the formset when
    /// the request asks for it. A failed call keeps the page and returns
    /// [`Event::Failed`].
    ///
    /// An open popup takes every input first and returns [`Event::Popup`] when
    /// the person selects one of its answers. An input that the popup does not
    /// answer changes nothing else on the page.
    pub fn input<P: Provider<Handle = H>>(&mut self, input: Input, provider: &mut P) -> Event {
        if let Some(popup) = self.popup.as_mut() {
            let before = popup.selected;
            let selection = popup.input(input);
            if let Some(selection) = selection {
                self.popup = None;
                return Event::Popup(selection);
            }
            return if popup.selected == before {
                Event::None
            } else {
                Event::Changed
            };
        }
        if self.editor.is_some() {
            return self.edit(input, provider);
        }
        if input == Input::Back {
            return self.back();
        }
        let entries = self.entries(provider);
        self.repair(&entries);
        let current_selection = self.selection();
        if matches!(input, Input::Up | Input::Down | Input::Home) {
            let selectable: Vec<_> = entries
                .iter()
                .filter(|e| e.row.enabled)
                .map(|e| e.source)
                .collect();
            let current = self
                .selection()
                .and_then(|s| selectable.iter().position(|i| *i == s))
                .unwrap_or(0);
            let next = match input {
                Input::Up if current == 0 => selectable.len().saturating_sub(1),
                Input::Up => current - 1,
                Input::Down if current + 1 < selectable.len() => current + 1,
                Input::Down => 0,
                _ => 0,
            };
            self.select(selectable.get(next).copied());
            return if self.selection() == current_selection {
                Event::None
            } else {
                Event::Changed
            };
        }
        if input != Input::Enter {
            return Event::None;
        }
        let Some(source) = self.selection() else {
            return Event::None;
        };
        if source == BACK_ROW {
            return self.back();
        }
        if self.pages.is_empty() {
            let set = &self.sets[source].1;
            if let Some(form) = set.forms.first() {
                self.pages.push(Page {
                    handle: self.sets[source].0,
                    guid: set.guid,
                    form: form.id,
                    selected: None,
                });
                self.message = None;
                return Event::Changed;
            }
            return Event::None;
        }
        let page = self.pages.last().expect("page exists");
        let (handle, guid, form) = (page.handle, page.guid, page.form);
        let (_, set) = self
            .sets
            .iter()
            .find(|s| s.0 == handle && s.1.guid == guid)
            .expect("page formset exists");
        let statement = &set.form(form).expect("page form exists").items[source].statement;
        if let Statement::Goto { form, .. } = statement {
            if set.form(*form).is_some() {
                self.pages.push(Page {
                    handle,
                    guid,
                    form: *form,
                    selected: None,
                });
                self.message = None;
                return Event::Changed;
            }
            return self.failure(Error {
                code: 0,
                reason: "The target form is unavailable".into(),
            });
        }
        let Some(question) = statement.question() else {
            return Event::None;
        };
        let id = question.id;
        if question.flags.contains(QuestionFlags::READ_ONLY) {
            // A read-only question is selectable only as a readable row.
            return Event::None;
        }
        if matches!(statement, Statement::Action { .. }) {
            let result = provider.action(handle, set, question);
            return self.result(result, handle, guid, form, id, provider);
        }
        let value = match provider.read(handle, set, question) {
            Ok(value) => value,
            Err(error) => return self.failure(error),
        };
        let kind = match (statement, value) {
            (Statement::Checkbox { .. }, Value::Bool(value)) => {
                let result = provider.write(handle, set, question, &Value::Bool(!value));
                return self.result(result, handle, guid, form, id, provider);
            }
            (Statement::Checkbox { .. }, Value::Number(value)) => {
                let negated = Value::Number(u64::from(value == 0));
                let result = provider.write(handle, set, question, &negated);
                return self.result(result, handle, guid, form, id, provider);
            }
            (Statement::OneOf { choices, .. }, Value::Number(value)) => EditorKind::Choice {
                choices: choices.iter().map(|c| c.text.clone()).collect(),
                selected: choices.iter().position(|c| c.value == value).unwrap_or(0),
            },
            (
                Statement::Numeric {
                    minimum, maximum, ..
                },
                Value::Number(value),
            ) => EditorKind::Number {
                text: value.to_string(),
                minimum: *minimum,
                maximum: *maximum,
            },
            (Statement::TextInput { maximum, .. }, Value::Text(text)) => EditorKind::Text {
                text,
                maximum: *maximum,
            },
            _ => {
                return self.failure(Error {
                    code: 0,
                    reason: "The provider returned an incompatible value".into(),
                });
            }
        };
        self.editor = Some((
            source,
            Editor {
                prompt: question.prompt.clone(),
                kind,
            },
        ));
        self.message = None;
        Event::Changed
    }

    /// Pops one page; at the formset list or a pinned form, the session exits.
    fn back(&mut self) -> Event {
        self.message = None;
        if self.pages.len() > self.base {
            self.pages.pop();
            Event::Changed
        } else {
            Event::Exit
        }
    }
    fn selection(&self) -> Option<usize> {
        self.pages.last().map_or(self.root, |p| p.selected)
    }
    fn select(&mut self, value: Option<usize>) {
        if let Some(page) = self.pages.last_mut() {
            page.selected = value;
        } else {
            self.root = value;
        }
    }
    fn repair(&mut self, entries: &[Entry]) {
        let old = self.selection();
        if !entries
            .iter()
            .any(|e| Some(e.source) == old && e.row.enabled)
        {
            self.select(
                entries
                    .iter()
                    .find(|e| e.row.enabled && old.is_none_or(|i| e.source >= i))
                    .or_else(|| entries.iter().find(|e| e.row.enabled))
                    .map(|e| e.source),
            );
        }
    }
    fn entries<P: Provider<Handle = H>>(&self, provider: &mut P) -> Vec<Entry> {
        let Some(page) = self.pages.last() else {
            return self
                .sets
                .iter()
                .enumerate()
                .map(|(source, (_, set))| Entry {
                    source,
                    row: Row {
                        prompt: set.title.clone(),
                        value: String::new(),
                        kind: RowKind::FormSet,
                        enabled: !set.forms.is_empty(),
                        uncertain: false,
                    },
                    help: set.help.clone(),
                })
                .collect();
        };
        let (handle, set) = self
            .sets
            .iter()
            .find(|s| s.0 == page.handle && s.1.guid == page.guid)
            .expect("page formset exists");
        let form = set.form(page.form).expect("page form exists");
        let mut entries: Vec<Entry> = form
            .items
            .iter()
            .enumerate()
            .filter_map(|(source, item)| {
                let (hidden, uncertain_hide) =
                    condition(item.suppress_if.as_ref(), *handle, set, provider);
                if hidden {
                    return None;
                }
                let (gray, uncertain_gray) =
                    condition(item.gray_out_if.as_ref(), *handle, set, provider);
                let (prompt, help, value, kind) = match &item.statement {
                    Statement::Subtitle { text } => {
                        (text.clone(), String::new(), String::new(), RowKind::Heading)
                    }
                    Statement::Text {
                        prompt,
                        help,
                        value,
                    } => (prompt.clone(), help.clone(), value.clone(), RowKind::Info),
                    Statement::Goto { prompt, help, .. } => {
                        (prompt.clone(), help.clone(), String::new(), RowKind::Link)
                    }
                    statement => {
                        let q = statement.question().expect("question statement");
                        let kind = match statement {
                            Statement::Action { .. } => RowKind::Action,
                            Statement::Checkbox { .. } => RowKind::Toggle,
                            Statement::OneOf { .. } => RowKind::Choice,
                            Statement::Numeric { .. } => RowKind::Number,
                            _ => RowKind::Text,
                        };
                        let value = if kind == RowKind::Action {
                            String::new()
                        } else {
                            provider
                                .read(*handle, set, q)
                                .map(|v| display_value(statement, v))
                                .unwrap_or_else(|_| "<unavailable>".into())
                        };
                        (
                            q.prompt.clone(),
                            q.help.clone(),
                            value,
                            if q.flags.contains(QuestionFlags::READ_ONLY) {
                                RowKind::Info
                            } else {
                                kind
                            },
                        )
                    }
                };
                Some(Entry {
                    source,
                    row: Row {
                        prompt,
                        value,
                        kind,
                        enabled: !gray
                            && kind != RowKind::Heading
                            && (self.readable_rows || kind != RowKind::Info),
                        uncertain: uncertain_hide || uncertain_gray,
                    },
                    help,
                })
            })
            .collect();
        // The pinned entry form keeps a Back row only when the session allows it.
        let entry = self.base > 0 && self.pages.len() == self.base;
        if self.back_row && (self.entry_back_row || !entry) {
            entries.push(Entry {
                source: BACK_ROW,
                row: Row {
                    prompt: String::from(BACK_PROMPT),
                    value: String::new(),
                    kind: RowKind::Back,
                    enabled: true,
                    uncertain: false,
                },
                help: String::from("Return to the previous page."),
            });
        }
        entries
    }
    fn failure(&mut self, error: Error) -> Event {
        self.message = Some(error.reason.clone());
        Event::Failed(error)
    }
    /// Acts on the request that a provider returned for one question.
    ///
    /// The browser commits or discards the scope that the request names,
    /// reconnects the controller, or leaves the form or the formset. A failed
    /// commit, discard or reconnect keeps the current page and shows the
    /// reason. [`ActionRequest::Reset`] keeps the page and asks the front end
    /// to reset the platform.
    fn result<P: Provider<Handle = H>>(
        &mut self,
        result: Result<ActionRequest, Error>,
        handle: H,
        guid: Guid,
        form: FormId,
        question: QuestionId,
        provider: &mut P,
    ) -> Event {
        let request = match result {
            Ok(request) => request,
            Err(error) => return self.failure(error),
        };
        if let Err(error) = self.commit(request, handle, guid, form, question, provider) {
            return self.failure(error);
        }
        if let Err(error) = self.revert(request, handle, guid, form, provider) {
            return self.failure(error);
        }
        if let Err(error) = Self::reconnect(request, handle, provider) {
            return self.failure(error);
        }
        self.message = match request {
            ActionRequest::Reset => {
                Some("A reset is required for the change to take effect".into())
            }
            _ => None,
        };
        if request.discards() {
            self.editor = None;
        }
        if request.leaves_formset() {
            self.pages.clear();
        } else if request.leaves_form() {
            if self.pages.len() > self.base {
                self.pages.pop();
            } else {
                self.pages.clear();
            }
        }
        Event::Response(request)
    }

    /// Commits the scope that the request names.
    fn commit<P: Provider<Handle = H>>(
        &self,
        request: ActionRequest,
        handle: H,
        guid: Guid,
        form: FormId,
        question: QuestionId,
        provider: &mut P,
    ) -> Result<(), Error> {
        let scope = match request {
            ActionRequest::Submit => Scope::FormSet,
            ActionRequest::FormApply | ActionRequest::FormSubmitExit => Scope::Form(form),
            ActionRequest::QuestionApply => Scope::Question(question),
            _ => return Ok(()),
        };
        let Some(index) = self.set_index(handle, guid) else {
            return Ok(());
        };
        provider.apply(handle, &self.sets[index].1, scope)
    }

    /// Discards the scope that the request names.
    fn revert<P: Provider<Handle = H>>(
        &self,
        request: ActionRequest,
        handle: H,
        guid: Guid,
        form: FormId,
        provider: &mut P,
    ) -> Result<(), Error> {
        let scope = match request {
            ActionRequest::Exit => Scope::FormSet,
            ActionRequest::FormDiscard | ActionRequest::FormDiscardExit => Scope::Form(form),
            _ => return Ok(()),
        };
        let Some(index) = self.set_index(handle, guid) else {
            return Ok(());
        };
        provider.discard(handle, &self.sets[index].1, scope)
    }

    /// Reconnects the controller when the request asks for it.
    fn reconnect<P: Provider<Handle = H>>(
        request: ActionRequest,
        handle: H,
        provider: &mut P,
    ) -> Result<(), Error> {
        if request == ActionRequest::Reconnect {
            provider.reconnect(handle)?;
        }
        Ok(())
    }

    /// Returns the index of the published formset with this identity.
    fn set_index(&self, handle: H, guid: Guid) -> Option<usize> {
        self.sets
            .iter()
            .position(|set| set.0 == handle && set.1.guid == guid)
    }
    fn edit<P: Provider<Handle = H>>(&mut self, input: Input, provider: &mut P) -> Event {
        if input == Input::Back {
            self.editor = None;
            self.message = None;
            return Event::Changed;
        }
        // The Back row is the last entry of a choice editor, so Enter on it
        // cancels without a write. Check it before the mutable borrow below.
        if input == Input::Enter
            && matches!(
                self.editor.as_ref().map(|(_, editor)| &editor.kind),
                Some(EditorKind::Choice { choices, selected }) if *selected == choices.len()
            )
        {
            self.editor = None;
            self.message = None;
            return Event::Changed;
        }
        let (source, editor) = self.editor.as_mut().expect("editor exists");
        if input != Input::Enter {
            match &mut editor.kind {
                EditorKind::Choice { choices, selected } => match input {
                    Input::Up | Input::Left => {
                        *selected = if *selected == 0 {
                            choices.len()
                        } else {
                            *selected - 1
                        };
                    }
                    Input::Down | Input::Right => {
                        *selected = if *selected < choices.len() {
                            *selected + 1
                        } else {
                            0
                        };
                    }
                    Input::Home => *selected = 0,
                    _ => return Event::None,
                },
                EditorKind::Number {
                    text,
                    minimum,
                    maximum,
                } => match input {
                    Input::Char(c) if c.is_ascii_digit() => text.push(c),
                    Input::Erase => {
                        text.pop();
                    }
                    Input::Up | Input::Right | Input::Down | Input::Left => {
                        let page = self.pages.last().expect("editor page exists");
                        let set = &self
                            .sets
                            .iter()
                            .find(|s| s.0 == page.handle && s.1.guid == page.guid)
                            .expect("editor formset exists")
                            .1;
                        let Statement::Numeric { step, .. } =
                            &set.form(page.form).expect("editor form exists").items[*source]
                                .statement
                        else {
                            unreachable!()
                        };
                        let step = (*step).max(1);
                        if let Ok(value) = text.parse::<u64>() {
                            let value = if matches!(input, Input::Up | Input::Right) {
                                value.saturating_add(step)
                            } else {
                                value.saturating_sub(step)
                            };
                            if value >= *minimum && value <= *maximum {
                                *text = value.to_string();
                            }
                        }
                    }
                    _ => return Event::None,
                },
                EditorKind::Text { text, maximum } => match input {
                    Input::Char(c)
                        if !c.is_control() && text.chars().count() < usize::from(*maximum) =>
                    {
                        text.push(c)
                    }
                    Input::Erase => {
                        text.pop();
                    }
                    _ => return Event::None,
                },
            }
            return Event::Changed;
        }
        let page = self.pages.last().expect("editor page exists");
        let (handle, guid, form) = (page.handle, page.guid, page.form);
        let (_, set) = self
            .sets
            .iter()
            .find(|s| s.0 == handle && s.1.guid == guid)
            .expect("editor formset exists");
        let statement = &set.form(form).expect("editor form exists").items[*source].statement;
        let value = match (&editor.kind, statement) {
            (EditorKind::Choice { selected, .. }, Statement::OneOf { choices, .. }) => {
                choices.get(*selected).map(|c| Value::Number(c.value))
            }
            (
                EditorKind::Number { text, .. },
                Statement::Numeric {
                    minimum,
                    maximum,
                    step,
                    ..
                },
            ) => text
                .parse::<u64>()
                .ok()
                .filter(|v| {
                    v >= minimum && v <= maximum && (*step == 0 || (v - minimum) % step == 0)
                })
                .map(Value::Number),
            (
                EditorKind::Text { text, .. },
                Statement::TextInput {
                    minimum, maximum, ..
                },
            ) => {
                let count = text.chars().count();
                (count >= usize::from(*minimum) && count <= usize::from(*maximum))
                    .then(|| Value::Text(text.clone()))
            }
            _ => None,
        };
        let Some(value) = value else {
            return self.failure(Error {
                code: 0,
                reason: "The value is outside the question's bounds or step".into(),
            });
        };
        let question = statement.question().expect("editor question exists");
        let id = question.id;
        let result = provider.write(handle, set, question, &value);
        if result.is_ok() {
            self.editor = None;
        }
        self.result(result, handle, guid, form, id, provider)
    }
}

fn display_value(statement: &Statement, value: Value) -> String {
    match value {
        Value::Bool(value) => if value { "On" } else { "Off" }.into(),
        Value::Number(value) => {
            if let Statement::Checkbox { .. } = statement {
                return if value == 0 { "Off" } else { "On" }.into();
            }
            if let Statement::OneOf { choices, .. } = statement
                && let Some(choice) = choices.iter().find(|c| c.value == value)
            {
                return choice.text.clone();
            }
            value.to_string()
        }
        Value::Text(text) => text,
        Value::Bytes(bytes) => {
            let mut text = String::new();
            for byte in bytes {
                text.push_str(&format!("{byte:02x}"));
            }
            text
        }
    }
}

fn read_question<H: Copy + Eq, P: Provider<Handle = H>>(
    id: QuestionId,
    handle: H,
    set: &FormSet,
    provider: &mut P,
) -> Option<Value> {
    let question = set
        .forms
        .iter()
        .flat_map(|f| &f.items)
        .filter_map(|i| i.statement.question())
        .find(|q| q.id == id)?;
    provider.read(handle, set, question).ok()
}
fn number(value: Value) -> Option<u64> {
    match value {
        Value::Bool(v) => Some(u64::from(v)),
        Value::Number(v) => Some(v),
        _ => None,
    }
}
fn condition<H: Copy + Eq, P: Provider<Handle = H>>(
    expr: Option<&Expr>,
    handle: H,
    set: &FormSet,
    provider: &mut P,
) -> (bool, bool) {
    match expr {
        None | Some(Expr::False) => (false, false),
        Some(Expr::True) => (true, false),
        Some(Expr::Unsupported) => (false, true),
        Some(Expr::Not(inner)) => {
            let (value, uncertain) = condition(Some(inner), handle, set, provider);
            (!value, uncertain)
        }
        Some(Expr::And(a, b) | Expr::Or(a, b)) => {
            let (a, ua) = condition(Some(a), handle, set, provider);
            let (b, ub) = condition(Some(b), handle, set, provider);
            (
                if matches!(expr, Some(Expr::And(..))) {
                    a && b
                } else {
                    a || b
                },
                ua || ub,
            )
        }
        Some(Expr::EqualValue { question, value }) => (
            read_question(*question, handle, set, provider)
                .and_then(number)
                .is_some_and(|v| v == *value),
            false,
        ),
        Some(Expr::EqualList { question, values }) => (
            read_question(*question, handle, set, provider)
                .and_then(number)
                .is_some_and(|v| values.contains(&v)),
            false,
        ),
        Some(Expr::EqualQuestion { first, second }) => {
            let a = read_question(*first, handle, set, provider);
            let b = read_question(*second, handle, set, provider);
            (a.is_some() && a == b, false)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::{Choice, Form, FormId, Item, PopupStyle, PopupType, Question, Storage, Width};
    use alloc::collections::BTreeMap;
    use alloc::{vec, vec::Vec};

    /// One lifecycle call that the session made on the provider.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        /// A commit of the scope.
        Apply(Scope),
        /// A discard of the scope.
        Discard(Scope),
    }

    #[derive(Default)]
    struct Memory {
        values: BTreeMap<u16, Value>,
        writes: Vec<(QuestionId, Value)>,
        write_handles: Vec<u32>,
        actions: Vec<QuestionId>,
        action_handles: Vec<u32>,
        responses: BTreeMap<u16, ActionRequest>,
        /// The value from before the first change of each question.
        originals: BTreeMap<u16, Value>,
        /// Lifecycle calls, in call order.
        lifecycle: Vec<Step>,
        /// The handles of the reconnect calls.
        reconnects: Vec<u32>,
        /// When set, every discard fails with this reason.
        discard_failure: Option<&'static str>,
    }

    impl Memory {
        fn put(&mut self, id: u16, value: Value) -> &mut Self {
            self.values.insert(id, value);
            self
        }

        fn respond(&mut self, id: u16, response: ActionRequest) -> &mut Self {
            self.responses.insert(id, response);
            self
        }

        /// Returns the question identifiers that a scope covers.
        fn scope_ids(set: &FormSet, scope: Scope) -> Vec<u16> {
            match scope {
                Scope::FormSet => set
                    .forms
                    .iter()
                    .flat_map(|form| form.items.iter())
                    .filter_map(|item| item.statement.question())
                    .map(|question| question.id.0)
                    .collect(),
                Scope::Form(form) => set.form(form).map_or_else(Vec::new, |form| {
                    form.items
                        .iter()
                        .filter_map(|item| item.statement.question())
                        .map(|question| question.id.0)
                        .collect()
                }),
                Scope::Question(id) => vec![id.0],
            }
        }
    }

    impl Provider for Memory {
        type Handle = u32;

        fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error> {
            Ok(Vec::new())
        }

        fn read(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
        ) -> Result<Value, Error> {
            Ok(self
                .values
                .get(&question.id.0)
                .cloned()
                .unwrap_or(Value::Number(0)))
        }

        fn write(
            &mut self,
            handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
            value: &Value,
        ) -> Result<ActionRequest, Error> {
            self.writes.push((question.id, value.clone()));
            self.write_handles.push(handle);
            // Keep the value from before the first change, as a browser does.
            if let Some(old) = self.values.get(&question.id.0).cloned() {
                self.originals.entry(question.id.0).or_insert(old);
            }
            self.values.insert(question.id.0, value.clone());
            Ok(self
                .responses
                .get(&question.id.0)
                .cloned()
                .unwrap_or(ActionRequest::None))
        }

        fn action(
            &mut self,
            handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
        ) -> Result<ActionRequest, Error> {
            self.actions.push(question.id);
            self.action_handles.push(handle);
            Ok(self
                .responses
                .get(&question.id.0)
                .cloned()
                .unwrap_or(ActionRequest::None))
        }

        fn apply(
            &mut self,
            _handle: Self::Handle,
            set: &FormSet,
            scope: Scope,
        ) -> Result<(), Error> {
            self.lifecycle.push(Step::Apply(scope));
            for id in Self::scope_ids(set, scope) {
                self.originals.remove(&id);
            }
            Ok(())
        }

        fn discard(
            &mut self,
            _handle: Self::Handle,
            set: &FormSet,
            scope: Scope,
        ) -> Result<(), Error> {
            self.lifecycle.push(Step::Discard(scope));
            if let Some(reason) = self.discard_failure {
                return Err(Error {
                    code: 1,
                    reason: reason.into(),
                });
            }
            for id in Self::scope_ids(set, scope) {
                if let Some(old) = self.originals.remove(&id) {
                    self.values.insert(id, old);
                }
            }
            Ok(())
        }

        fn reconnect(&mut self, handle: Self::Handle) -> Result<(), Error> {
            self.reconnects.push(handle);
            Ok(())
        }
    }

    fn question(id: u16, prompt: &str) -> Question {
        Question {
            id: QuestionId(id),
            prompt: prompt.into(),
            help: String::new(),
            storage: Storage::None,
            flags: QuestionFlags(0),
        }
    }

    fn item(statement: Statement) -> Item {
        Item {
            statement,
            suppress_if: None,
            gray_out_if: None,
        }
    }

    fn form(id: u16, title: &str, items: Vec<Item>) -> Form {
        Form {
            id: FormId(id),
            title: title.into(),
            items,
        }
    }

    fn formset(guid: u8, title: &str, forms: Vec<Form>) -> FormSet {
        FormSet {
            guid: [guid; 16],
            title: title.into(),
            help: String::new(),
            class_guids: Vec::new(),
            varstores: Vec::new(),
            forms,
        }
    }

    fn page() -> FormSet {
        formset(
            0x10,
            "Setup",
            vec![form(
                1,
                "Main",
                vec![
                    item(Statement::Subtitle {
                        text: "State".into(),
                    }),
                    item(Statement::Text {
                        prompt: "Mode".into(),
                        help: String::new(),
                        value: "Normal".into(),
                    }),
                    item(Statement::Action {
                        question: question(5, "Run test"),
                    }),
                    item(Statement::Checkbox {
                        question: question(1, "Enabled"),
                        default: false,
                    }),
                    item(Statement::OneOf {
                        question: question(2, "Level"),
                        width: Width::U8,
                        choices: vec![
                            Choice {
                                text: "Low".into(),
                                value: 10,
                                default: true,
                            },
                            Choice {
                                text: "High".into(),
                                value: 20,
                                default: false,
                            },
                        ],
                    }),
                    item(Statement::Numeric {
                        question: question(3, "Size"),
                        width: Width::U16,
                        minimum: 10,
                        maximum: 100,
                        step: 5,
                    }),
                    item(Statement::TextInput {
                        question: question(4, "Name"),
                        minimum: 0,
                        maximum: 8,
                    }),
                ],
            )],
        )
    }

    /// The test page without the Back row, so row indices match its items.
    fn session() -> Session<u32> {
        Session::new(vec![(7, page())]).with_back_row(false)
    }

    /// Moves to the page of `page()` and returns the session.
    fn entered(memory: &mut Memory) -> Session<u32> {
        let mut session = session();
        assert_eq!(session.input(Input::Enter, memory), Event::Changed);
        session
    }

    #[test]
    fn root_lists_formsets_sorted_by_title() {
        let mut session = Session::new(vec![
            (1, formset(1, "Zeta", vec![form(1, "Z", Vec::new())])),
            (2, formset(2, "Alpha", vec![form(1, "A", Vec::new())])),
        ]);
        let mut memory = Memory::default();
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Formsets");
        let titles: Vec<_> = view.rows.iter().map(|row| row.prompt.as_str()).collect();
        assert_eq!(titles, ["Alpha", "Zeta"]);
        assert_eq!(view.rows[0].kind, RowKind::FormSet);
        assert_eq!(view.selected, Some(0));
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).selected, Some(1));
    }

    #[test]
    fn navigation_skips_hidden_disabled_and_static_rows() {
        let mut hidden = item(Statement::Numeric {
            question: question(2, "Hidden"),
            width: Width::U8,
            minimum: 0,
            maximum: 9,
            step: 1,
        });
        hidden.suppress_if = Some(Expr::True);
        let mut gray = item(Statement::TextInput {
            question: question(3, "Gray"),
            minimum: 0,
            maximum: 4,
        });
        gray.gray_out_if = Some(Expr::True);
        let set = formset(
            0x20,
            "Skip",
            vec![form(
                1,
                "Skip",
                vec![
                    item(Statement::Subtitle {
                        text: "Head".into(),
                    }),
                    item(Statement::Text {
                        prompt: "Info".into(),
                        help: String::new(),
                        value: "v".into(),
                    }),
                    item(Statement::Checkbox {
                        question: question(1, "Toggle"),
                        default: false,
                    }),
                    hidden,
                    gray,
                    item(Statement::Numeric {
                        question: question(4, "Last"),
                        width: Width::U8,
                        minimum: 0,
                        maximum: 9,
                        step: 1,
                    }),
                ],
            )],
        );
        let mut session = Session::new(vec![(1, set)]).with_back_row(false);
        let mut memory = Memory::default();
        session.input(Input::Enter, &mut memory);
        let view = session.view(&mut memory);
        let prompts: Vec<_> = view.rows.iter().map(|row| row.prompt.as_str()).collect();
        assert_eq!(prompts, ["Head", "Info", "Toggle", "Gray", "Last"]);
        assert!(!view.rows[0].enabled && !view.rows[1].enabled);
        assert!(view.rows[2].enabled);
        assert!(!view.rows[3].enabled, "grayed rows are not selectable");
        assert_eq!(view.selected, Some(2));
        session.input(Input::Down, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(4));
        session.input(Input::Down, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(2));
        session.input(Input::Up, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(4));
        session.input(Input::Home, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(2));
    }

    #[test]
    fn enter_on_action_runs_provider_once() {
        let mut memory = Memory::default();
        memory.respond(5, ActionRequest::FormApply);
        let mut session = entered(&mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(2));
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::FormApply)
        );
        assert_eq!(memory.actions, [QuestionId(5)]);
        assert_eq!(memory.lifecycle, [Step::Apply(Scope::Form(FormId(1)))]);
        assert_eq!(session.view(&mut memory).title, "Main");
    }

    #[test]
    fn toggle_writes_negation() {
        let mut memory = Memory::default();
        memory
            .put(1, Value::Bool(false))
            .respond(1, ActionRequest::QuestionApply);
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(3));
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::QuestionApply)
        );
        assert_eq!(memory.writes, [(QuestionId(1), Value::Bool(true))]);
        assert_eq!(
            memory.lifecycle,
            [Step::Apply(Scope::Question(QuestionId(1)))]
        );
        assert_eq!(session.view(&mut memory).rows[3].value, "On");
    }

    #[test]
    fn numeric_editor_rejects_invalid_values_without_writing() {
        let mut memory = Memory::default();
        memory.put(3, Value::Number(50));
        let mut session = entered(&mut memory);
        for _ in 0..3 {
            session.input(Input::Down, &mut memory);
        }
        assert_eq!(session.view(&mut memory).selected, Some(5));
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        assert_eq!(
            session.view(&mut memory).editor,
            Some(Editor {
                prompt: "Size".into(),
                kind: EditorKind::Number {
                    text: "50".into(),
                    minimum: 10,
                    maximum: 100
                },
            })
        );
        session.input(Input::Erase, &mut memory);
        session.input(Input::Erase, &mut memory);
        session.input(Input::Char('7'), &mut memory);
        assert!(matches!(
            session.input(Input::Enter, &mut memory),
            Event::Failed(_)
        ));
        assert!(session.view(&mut memory).editor.is_some());
        assert!(session.view(&mut memory).message.is_some());
        session.input(Input::Erase, &mut memory);
        session.input(Input::Char('1'), &mut memory);
        session.input(Input::Char('2'), &mut memory);
        assert!(matches!(
            session.input(Input::Enter, &mut memory),
            Event::Failed(_)
        ));
        assert!(session.view(&mut memory).editor.is_some());
        session.input(Input::Erase, &mut memory);
        session.input(Input::Erase, &mut memory);
        session.input(Input::Char('1'), &mut memory);
        session.input(Input::Char('5'), &mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.writes, [(QuestionId(3), Value::Number(15))]);
        assert!(session.view(&mut memory).editor.is_none());
    }

    #[test]
    fn choice_editor_commits_selected_value() {
        let mut memory = Memory::default();
        memory.put(2, Value::Number(10));
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Down, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(4));
        session.input(Input::Enter, &mut memory);
        let view = session.view(&mut memory);
        assert_eq!(view.rows[4].value, "Low");
        assert_eq!(
            view.editor,
            Some(Editor {
                prompt: "Level".into(),
                kind: EditorKind::Choice {
                    choices: vec!["Low".into(), "High".into()],
                    selected: 0
                },
            })
        );
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.writes, [(QuestionId(2), Value::Number(20))]);
        assert_eq!(session.view(&mut memory).rows[4].value, "High");
    }

    /// Returns the pending index of the open choice editor.
    ///
    /// `choices.len()` is the Back row, which cancels instead of committing.
    fn choice_index(session: &mut Session<u32>, memory: &mut Memory) -> usize {
        match session.view(memory).editor {
            Some(Editor {
                kind: EditorKind::Choice { selected, .. },
                ..
            }) => selected,
            other => panic!("no choice editor: {other:?}"),
        }
    }

    #[test]
    fn choice_editor_wraps_through_the_back_row() {
        let mut memory = Memory::default();
        memory.put(2, Value::Number(10));
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Enter, &mut memory);
        assert_eq!(choice_index(&mut session, &mut memory), 0);
        // Up from the first choice reaches Back, and Up again the last choice.
        session.input(Input::Up, &mut memory);
        assert_eq!(
            choice_index(&mut session, &mut memory),
            2,
            "Back follows the choices"
        );
        session.input(Input::Up, &mut memory);
        assert_eq!(choice_index(&mut session, &mut memory), 1);
        session.input(Input::Down, &mut memory);
        assert_eq!(choice_index(&mut session, &mut memory), 2);
        session.input(Input::Down, &mut memory);
        assert_eq!(choice_index(&mut session, &mut memory), 0);
    }

    #[test]
    fn enter_on_the_back_row_cancels_without_writing() {
        let mut memory = Memory::default();
        memory.put(2, Value::Number(10));
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Enter, &mut memory);
        session.input(Input::Up, &mut memory);
        assert_eq!(choice_index(&mut session, &mut memory), 2);
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        let view = session.view(&mut memory);
        assert!(view.editor.is_none());
        assert!(memory.writes.is_empty(), "a cancel never commits");
        assert_eq!(view.rows[4].value, "Low", "the committed value stays");
    }

    #[test]
    fn choice_without_options_cancels_without_writing() {
        let mut memory = Memory::default();
        let set = formset(
            0x30,
            "Empty",
            vec![form(
                1,
                "Empty",
                vec![item(Statement::OneOf {
                    question: question(2, "Mode"),
                    width: Width::U8,
                    choices: Vec::new(),
                })],
            )],
        );
        let mut session = Session::new(vec![(1, set)]).with_back_row(false);
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        assert_eq!(
            session.view(&mut memory).editor,
            Some(Editor {
                prompt: "Mode".into(),
                kind: EditorKind::Choice {
                    choices: Vec::new(),
                    selected: 0,
                },
            })
        );
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        assert!(session.view(&mut memory).editor.is_none());
        assert!(memory.writes.is_empty());
    }

    #[test]
    fn back_cancels_editor_and_pops_to_root() {
        let mut memory = Memory::default();
        let mut session = session();
        assert_eq!(session.input(Input::Back, &mut memory), Event::Exit);
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        session.input(Input::Down, &mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Enter, &mut memory);
        assert!(session.view(&mut memory).editor.is_some());
        assert_eq!(session.input(Input::Back, &mut memory), Event::Changed);
        assert!(session.view(&mut memory).editor.is_none());
        assert!(memory.writes.is_empty());
        assert_eq!(session.input(Input::Back, &mut memory), Event::Changed);
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Formsets");
        assert_eq!(view.path, Vec::<String>::new());
        assert_eq!(session.input(Input::Back, &mut memory), Event::Exit);
    }

    #[test]
    fn reset_and_exit_responses_apply() {
        let mut memory = Memory::default();
        memory.respond(1, ActionRequest::Reset);
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::Reset)
        );
        let message = session.view(&mut memory).message.unwrap_or_default();
        assert!(message.contains("reset"), "message was {message:?}");
        assert_eq!(session.view(&mut memory).title, "Main");

        let mut memory = Memory::default();
        memory.respond(5, ActionRequest::Exit);
        let mut session = entered(&mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::Exit)
        );
        assert_eq!(memory.lifecycle, [Step::Discard(Scope::FormSet)]);
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Formsets");
        assert_eq!(view.rows.len(), 1);
    }

    /// A formset with a link to a second form, for the form-level requests.
    fn linked() -> FormSet {
        formset(
            0x30,
            "Linked",
            vec![
                form(
                    1,
                    "First",
                    vec![
                        item(Statement::Goto {
                            prompt: "More".into(),
                            help: String::new(),
                            form: FormId(2),
                            question: None,
                        }),
                        item(Statement::Action {
                            question: question(5, "First run"),
                        }),
                    ],
                ),
                form(
                    2,
                    "Second",
                    vec![
                        item(Statement::Checkbox {
                            question: question(1, "Shared switch"),
                            default: false,
                        }),
                        item(Statement::Action {
                            question: question(6, "Second run"),
                        }),
                    ],
                ),
            ],
        )
    }

    /// Opens the second form of `linked()`.
    fn entered_second(memory: &mut Memory) -> Session<u32> {
        let mut session = Session::new(vec![(7, linked())]).with_back_row(false);
        assert_eq!(session.input(Input::Enter, memory), Event::Changed);
        assert_eq!(session.input(Input::Enter, memory), Event::Changed);
        assert_eq!(session.view(memory).title, "Second");
        session
    }

    #[test]
    fn form_apply_commits_and_keeps_the_form() {
        let mut memory = Memory::default();
        memory
            .put(1, Value::Bool(false))
            .respond(1, ActionRequest::None);
        let mut session = entered_second(&mut memory);

        // The person changes a value on the second form.
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.values.get(&1), Some(&Value::Bool(true)));

        // FormApply commits the change and keeps the form open.
        memory.respond(6, ActionRequest::FormApply);
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::FormApply)
        );
        assert_eq!(memory.lifecycle, [Step::Apply(Scope::Form(FormId(2)))]);
        assert_eq!(session.view(&mut memory).title, "Second");
        assert_eq!(session.view(&mut memory).rows[0].value, "On");
    }

    #[test]
    fn form_discard_restores_the_values_of_the_form() {
        let mut memory = Memory::default();
        memory
            .put(1, Value::Bool(false))
            .respond(1, ActionRequest::None);
        let mut session = entered_second(&mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.values.get(&1), Some(&Value::Bool(true)));

        memory.respond(6, ActionRequest::FormDiscard);
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::FormDiscard)
        );
        assert_eq!(memory.lifecycle, [Step::Discard(Scope::Form(FormId(2)))]);
        assert_eq!(memory.values.get(&1), Some(&Value::Bool(false)));
        assert_eq!(session.view(&mut memory).title, "Second");
        assert_eq!(session.view(&mut memory).rows[0].value, "Off");
    }

    #[test]
    fn form_exit_requests_leave_the_form_after_the_scope_call() {
        for (request, step) in [
            (
                ActionRequest::FormSubmitExit,
                Step::Apply(Scope::Form(FormId(2))),
            ),
            (
                ActionRequest::FormDiscardExit,
                Step::Discard(Scope::Form(FormId(2))),
            ),
        ] {
            let mut memory = Memory::default();
            memory
                .put(1, Value::Bool(false))
                .respond(1, ActionRequest::None);
            let mut session = entered_second(&mut memory);
            session.input(Input::Enter, &mut memory);
            memory.respond(6, request);
            session.input(Input::Down, &mut memory);
            assert_eq!(
                session.input(Input::Enter, &mut memory),
                Event::Response(request)
            );
            assert_eq!(memory.lifecycle, [step]);
            assert_eq!(session.view(&mut memory).title, "First");
            let restored = request == ActionRequest::FormDiscardExit;
            assert_eq!(
                memory.values.get(&1),
                Some(&Value::Bool(!restored)),
                "request {request:?}"
            );
        }
    }

    #[test]
    fn submit_commits_the_formset_and_leaves_it() {
        let mut memory = Memory::default();
        memory.respond(5, ActionRequest::Submit);
        let mut session = entered(&mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::Submit)
        );
        assert_eq!(memory.lifecycle, [Step::Apply(Scope::FormSet)]);
        assert_eq!(session.view(&mut memory).title, "Formsets");
    }

    #[test]
    fn reconnect_reaches_the_provider_and_keeps_the_page() {
        let mut memory = Memory::default();
        memory.respond(5, ActionRequest::Reconnect);
        let mut session = entered(&mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::Reconnect)
        );
        assert_eq!(memory.reconnects, [7]);
        assert_eq!(session.view(&mut memory).title, "Main");
        assert!(session.view(&mut memory).message.is_none());
    }

    #[test]
    fn failed_discard_keeps_the_form_and_shows_the_reason() {
        let mut memory = Memory {
            discard_failure: Some("the driver refused the discard"),
            ..Memory::default()
        };
        memory.respond(5, ActionRequest::FormDiscard);
        let mut session = entered(&mut memory);
        match session.input(Input::Enter, &mut memory) {
            Event::Failed(error) => assert!(error.reason.contains("refused"), "{}", error.reason),
            event => panic!("unexpected event {event:?}"),
        }
        assert_eq!(session.view(&mut memory).title, "Main");
        assert_eq!(
            session.view(&mut memory).message.as_deref(),
            Some("the driver refused the discard")
        );
    }

    #[test]
    fn suppress_if_follows_controlling_question() {
        let mut dependent = item(Statement::Numeric {
            question: question(2, "Extra"),
            width: Width::U8,
            minimum: 0,
            maximum: 9,
            step: 1,
        });
        dependent.suppress_if = Some(Expr::EqualValue {
            question: QuestionId(1),
            value: 1,
        });
        let set = formset(
            0x30,
            "Condition",
            vec![form(
                1,
                "Condition",
                vec![
                    item(Statement::Checkbox {
                        question: question(1, "Control"),
                        default: false,
                    }),
                    dependent,
                ],
            )],
        );
        let mut session = Session::new(vec![(1, set)]).with_back_row(false);
        let mut memory = Memory::default();
        session.input(Input::Enter, &mut memory);
        assert_eq!(session.view(&mut memory).rows.len(), 2);
        assert_eq!(session.view(&mut memory).selected, Some(0));
        session.input(Input::Enter, &mut memory);
        let view = session.view(&mut memory);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].prompt, "Control");
        assert_eq!(view.selected, Some(0));
    }

    #[test]
    fn unsupported_condition_keeps_row_uncertain() {
        let mut dependent = item(Statement::Numeric {
            question: question(2, "Maybe"),
            width: Width::U8,
            minimum: 0,
            maximum: 9,
            step: 1,
        });
        dependent.suppress_if = Some(Expr::Unsupported);
        let set = formset(
            0x40,
            "Uncertain",
            vec![form(1, "Uncertain", vec![dependent])],
        );
        let mut session = Session::new(vec![(1, set)]).with_back_row(false);
        let mut memory = Memory::default();
        session.input(Input::Enter, &mut memory);
        let view = session.view(&mut memory);
        assert_eq!(view.rows.len(), 1);
        assert!(view.rows[0].uncertain);
        assert!(view.rows[0].enabled);
        assert_eq!(view.selected, Some(0));
    }

    #[test]
    fn replace_keeps_position_when_form_survives() {
        let set = formset(
            0x50,
            "Stack",
            vec![
                form(
                    1,
                    "Main",
                    vec![item(Statement::Goto {
                        prompt: "More".into(),
                        help: String::new(),
                        form: FormId(2),
                        question: None,
                    })],
                ),
                form(
                    2,
                    "Extra",
                    vec![item(Statement::Checkbox {
                        question: question(1, "Flag"),
                        default: false,
                    })],
                ),
            ],
        );
        let mut session = Session::new(vec![(1, set.clone())]).with_back_row(false);
        let mut memory = Memory::default();
        session.input(Input::Enter, &mut memory);
        assert_eq!(session.view(&mut memory).title, "Main");
        session.input(Input::Enter, &mut memory);
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Extra");
        assert_eq!(view.path, ["Formsets", "Main"]);
        assert_eq!(view.selected, Some(0));
        session.replace(vec![(1, set)]);
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Extra");
        assert_eq!(view.selected, Some(0));
        session.replace(vec![(
            1,
            formset(0x60, "Other", vec![form(1, "Other", Vec::new())]),
        )]);
        assert_eq!(session.view(&mut memory).title, "Formsets");
    }

    /// Selects the last navigable row through wraparound.
    fn to_last(session: &mut Session<u32>, memory: &mut Memory) {
        session.input(Input::Home, memory);
        session.input(Input::Up, memory);
    }

    #[test]
    fn back_row_ends_each_page_and_returns_to_the_previous_page() {
        let mut memory = Memory::default();
        let mut session = Session::new(vec![(7, page())]);
        let root = session.view(&mut memory);
        assert!(root.rows.iter().all(|row| row.kind != RowKind::Back));
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        let view = session.view(&mut memory);
        assert_eq!(view.rows.last().map(|row| row.kind), Some(RowKind::Back));
        // The first real item keeps the default selection.
        assert_ne!(view.selected, Some(view.rows.len() - 1));
        to_last(&mut session, &mut memory);
        let view = session.view(&mut memory);
        assert_eq!(view.selected, Some(view.rows.len() - 1));
        // Down wraps from the Back row to the first navigable row, and Up
        // wraps back again, so a phone reaches every row with two keys.
        let last = view.rows.len() - 1;
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).selected, Some(2));
        assert_eq!(session.input(Input::Up, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).selected, Some(last));
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).title, "Formsets");
        assert!(memory.writes.is_empty());
    }

    #[test]
    fn entered_formset_is_pinned_and_back_exits() {
        let mut memory = Memory::default();
        let other = formset(0x20, "Other", vec![form(1, "Other", Vec::new())]);
        let mut session = Session::new(vec![(1, other), (7, page())]);
        assert!(!session.enter(&[0x99; 16]));
        assert_eq!(session.view(&mut memory).title, "Formsets");
        assert!(session.enter(&[0x10; 16]));
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Main");
        assert!(view.path.is_empty());
        assert_eq!(session.input(Input::Back, &mut memory), Event::Exit);
        // The Back row of the pinned form exits too.
        to_last(&mut session, &mut memory);
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Exit);
    }

    #[test]
    fn entry_back_row_only_hides_the_pinned_entry_row() {
        let set = formset(
            0x60,
            "Pinned",
            vec![
                form(
                    1,
                    "Main",
                    vec![item(Statement::Goto {
                        prompt: "More".into(),
                        help: String::new(),
                        form: FormId(2),
                        question: None,
                    })],
                ),
                form(
                    2,
                    "Extra",
                    vec![item(Statement::Checkbox {
                        question: question(1, "Flag"),
                        default: false,
                    })],
                ),
            ],
        );
        let mut memory = Memory::default();
        let mut session = Session::new(vec![(1, set.clone())]).with_entry_back_row(false);
        assert!(session.enter(&[0x60; 16]));
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Main");
        assert!(view.rows.iter().all(|row| row.kind != RowKind::Back));
        // A page above the pinned entry form keeps the ordinary Back row.
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Extra");
        assert_eq!(view.rows.last().map(|row| row.kind), Some(RowKind::Back));
        // Back returns to the pinned form, which has no Back row, and Back
        // on the pinned form exits.
        assert_eq!(session.input(Input::Back, &mut memory), Event::Changed);
        assert!(
            session
                .view(&mut memory)
                .rows
                .iter()
                .all(|row| row.kind != RowKind::Back)
        );
        assert_eq!(session.input(Input::Back, &mut memory), Event::Exit);
        // The default keeps the Back row on the pinned form.
        let mut memory = Memory::default();
        let mut session = Session::new(vec![(1, set)]);
        assert!(session.enter(&[0x60; 16]));
        assert_eq!(
            session.view(&mut memory).rows.last().map(|row| row.kind),
            Some(RowKind::Back)
        );
    }

    #[test]
    fn readable_rows_select_information_without_an_effect() {
        let set = formset(
            0x61,
            "Facts",
            vec![form(
                1,
                "Facts",
                vec![
                    item(Statement::Subtitle {
                        text: "Group".into(),
                    }),
                    item(Statement::Text {
                        prompt: "Fact".into(),
                        help: "About the fact.".into(),
                        value: "Known".into(),
                    }),
                    item(Statement::Checkbox {
                        question: Question {
                            flags: QuestionFlags::READ_ONLY,
                            ..question(1, "Locked")
                        },
                        default: false,
                    }),
                ],
            )],
        );
        let mut memory = Memory::default();
        memory.put(1, Value::Bool(true));
        let mut session = Session::new(vec![(1, set.clone())]).with_readable_rows(true);
        assert!(session.enter(&[0x61; 16]));
        let view = session.view(&mut memory);
        assert_eq!(view.selected, Some(1), "the heading stays unselectable");
        assert_eq!(view.help, "About the fact.");
        assert_eq!(session.input(Input::Enter, &mut memory), Event::None);
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).rows[2].kind, RowKind::Info);
        // Enter on a read-only question opens no editor and writes nothing.
        assert_eq!(session.input(Input::Enter, &mut memory), Event::None);
        assert!(session.view(&mut memory).editor.is_none());
        assert!(memory.writes.is_empty());
        // The default keeps information rows out of the selection.
        let mut session = Session::new(vec![(1, set)]);
        assert!(session.enter(&[0x61; 16]));
        assert_eq!(session.view(&mut memory).selected, Some(3), "only Back");
    }

    #[test]
    fn duplicate_formset_guids_keep_provider_identity() {
        // Two providers publish one GUID. Their forms differ, so a lookup by
        // GUID alone shows the wrong page and routes to the wrong provider.
        let alpha = formset(
            0x70,
            "Alpha",
            vec![form(
                1,
                "Alpha Main",
                vec![
                    item(Statement::Action {
                        question: question(5, "Alpha run"),
                    }),
                    item(Statement::Checkbox {
                        question: question(1, "Alpha flag"),
                        default: false,
                    }),
                ],
            )],
        );
        let beta = formset(
            0x70,
            "Beta",
            vec![
                form(
                    1,
                    "Beta Main",
                    vec![
                        item(Statement::Goto {
                            prompt: "More".into(),
                            help: String::new(),
                            form: FormId(2),
                            question: None,
                        }),
                        item(Statement::Action {
                            question: question(6, "Beta run"),
                        }),
                        item(Statement::Checkbox {
                            question: question(1, "Beta flag"),
                            default: false,
                        }),
                    ],
                ),
                form(
                    2,
                    "Beta Extra",
                    vec![item(Statement::Checkbox {
                        question: question(7, "Beta extra flag"),
                        default: false,
                    })],
                ),
            ],
        );
        let mut memory = Memory::default();
        let mut session =
            Session::new(vec![(2, beta.clone()), (1, alpha.clone())]).with_back_row(false);
        let view = session.view(&mut memory);
        let titles: Vec<_> = view.rows.iter().map(|row| row.prompt.as_str()).collect();
        assert_eq!(titles, ["Alpha", "Beta"]);
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).title, "Beta Main");
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Beta Extra");
        assert_eq!(view.path, ["Formsets", "Beta Main"]);

        // A refresh republishes both formsets with the same GUID. The pages
        // stay on the handle that opened them, and show the refreshed forms.
        let refreshed = formset(
            0x70,
            "Beta",
            vec![
                form(
                    1,
                    "Beta Main",
                    vec![
                        item(Statement::Goto {
                            prompt: "More".into(),
                            help: String::new(),
                            form: FormId(2),
                            question: None,
                        }),
                        item(Statement::Action {
                            question: question(6, "Beta run"),
                        }),
                        item(Statement::Checkbox {
                            question: question(1, "Beta flag"),
                            default: false,
                        }),
                        item(Statement::Action {
                            question: question(8, "Beta added"),
                        }),
                    ],
                ),
                form(
                    2,
                    "Beta Extra Refreshed",
                    vec![item(Statement::Checkbox {
                        question: question(7, "Beta extra flag"),
                        default: false,
                    })],
                ),
            ],
        );
        session.replace(vec![(2, refreshed), (1, alpha.clone())]);
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Beta Extra Refreshed");
        assert_eq!(view.path, ["Formsets", "Beta Main"]);
        assert_eq!(view.selected, Some(0));

        // Actions and writes reach the provider that owns the open pages.
        assert_eq!(session.input(Input::Back, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).title, "Beta Main");
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(session.view(&mut memory).selected, Some(1));
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.actions, [QuestionId(6)]);
        assert_eq!(memory.action_handles, [2]);
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.writes, [(QuestionId(1), Value::Number(1))]);
        assert_eq!(memory.write_handles, [2]);
        assert_eq!(session.view(&mut memory).rows[2].value, "On");

        // `enter` picks the first published formset with the GUID, and the
        // pinned session keeps that handle.
        let mut memory = Memory::default();
        let mut pinned = Session::new(vec![(2, beta), (1, alpha)]).with_back_row(false);
        assert!(pinned.enter(&[0x70; 16]));
        let view = pinned.view(&mut memory);
        assert_eq!(view.title, "Alpha Main");
        assert!(view.path.is_empty());
        assert_eq!(pinned.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(
            pinned.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.writes, [(QuestionId(1), Value::Number(1))]);
        assert_eq!(memory.write_handles, [1]);
    }

    #[test]
    fn popup_takes_every_input_and_keeps_the_page() {
        let mut memory = Memory::default();
        let mut session = entered(&mut memory);
        let before = session.view(&mut memory);
        session.open_popup(Popup::new(
            PopupStyle::Warning,
            PopupType::YesNo,
            "Erase the log?",
        ));
        assert_eq!(
            session.view(&mut memory).popup.map(|popup| popup.message),
            Some(String::from("Erase the log?"))
        );
        // A warning starts on No, so two moves return to No.
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(session.input(Input::Down, &mut memory), Event::Changed);
        assert_eq!(session.input(Input::Char('q'), &mut memory), Event::None);
        assert_eq!(session.view(&mut memory).selected, before.selected);
        assert_eq!(memory.writes, []);
        assert_eq!(memory.actions, []);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Popup(PopupSelection::No)
        );
        let after = session.view(&mut memory);
        assert!(after.popup.is_none());
        assert_eq!(after.selected, before.selected);
        assert_eq!(after.title, before.title);
    }

    #[test]
    fn popup_cancels_only_a_type_that_offers_cancel() {
        let mut memory = Memory::default();
        let mut session = entered(&mut memory);
        session.open_popup(Popup::new(PopupStyle::Info, PopupType::Ok, "Stored."));
        assert_eq!(session.input(Input::Back, &mut memory), Event::None);
        assert!(session.view(&mut memory).popup.is_some());
        assert_eq!(
            session.input(Input::Char('o'), &mut memory),
            Event::Popup(PopupSelection::Ok)
        );

        session.open_popup(Popup::new(
            PopupStyle::Error,
            PopupType::YesNoCancel,
            "Keep the change?",
        ));
        assert_eq!(
            session.input(Input::Back, &mut memory),
            Event::Popup(PopupSelection::Cancel)
        );
    }

    #[test]
    fn a_second_popup_replaces_the_first() {
        let mut memory = Memory::default();
        let mut session = entered(&mut memory);
        session.open_popup(Popup::new(PopupStyle::Info, PopupType::Ok, "First"));
        session.open_popup(Popup::new(
            PopupStyle::Error,
            PopupType::Ok,
            "Second failure",
        ));
        let popup = session.view(&mut memory).popup.expect("a popup is open");
        assert_eq!(popup.message, "Second failure");
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Popup(PopupSelection::Ok)
        );
    }

    #[test]
    fn popup_leaves_an_open_editor_unchanged() {
        let mut memory = Memory::default();
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        session.input(Input::Down, &mut memory);
        assert_eq!(session.input(Input::Enter, &mut memory), Event::Changed);
        let editor = session.view(&mut memory).editor;
        assert!(editor.is_some(), "the choice editor is open");
        session.open_popup(Popup::new(PopupStyle::Info, PopupType::Ok, "Note"));
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Popup(PopupSelection::Ok)
        );
        assert_eq!(session.view(&mut memory).editor, editor);
        assert_eq!(memory.writes, []);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(ActionRequest::None)
        );
        assert_eq!(memory.writes, [(QuestionId(2), Value::Number(10))]);
    }
}
