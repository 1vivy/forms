//! Provider-backed navigation and editing.

use crate::{
    Error, Expr, FormId, FormSet, Guid, Provider, QuestionFlags, QuestionId, Response, Statement,
    Value,
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
    /// A provider returned a request.
    Response(Response),
    /// A provider or validation operation failed.
    Failed(Error),
    /// The user left the root page.
    Exit,
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
        /// The selected choice index.
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
}

#[derive(Clone)]
struct Page {
    guid: Guid,
    form: FormId,
    selected: Option<usize>,
}

struct Entry {
    source: usize,
    row: Row,
    help: String,
}

/// Navigation and editing state. Values remain owned by the provider.
///
/// # Examples
///
/// ```
/// let _session = forms::Session::<u32>::new(Vec::new());
/// ```
pub struct Session<H> {
    sets: Vec<(H, FormSet)>,
    pages: Vec<Page>,
    root: Option<usize>,
    editor: Option<(usize, Editor)>,
    message: Option<String>,
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
        }
    }

    /// Replaces published forms. Keeps surviving pages and their selection.
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
        let root_guid = self.root.and_then(|i| self.sets.get(i)).map(|s| s.1.guid);
        sets.sort_by(|a, b| a.1.title.cmp(&b.1.title));
        self.root = root_guid.and_then(|guid| sets.iter().position(|s| s.1.guid == guid));
        let valid = self
            .pages
            .iter()
            .take_while(|page| {
                sets.iter()
                    .any(|(_, set)| set.guid == page.guid && set.form(page.form).is_some())
            })
            .count();
        self.pages.truncate(valid);
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
        for page in &self.pages {
            path.push(title);
            title = self
                .sets
                .iter()
                .find(|s| s.1.guid == page.guid)
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
        }
    }

    /// Applies one input and returns navigation or provider effects.
    ///
    /// With an editor open, invalid values keep the editor open, set the
    /// message and return [`Event::Failed`] without calling the provider.
    /// Outside an editor, Back pops the form stack and returns
    /// [`Event::Exit`] at the root.
    pub fn input<P: Provider<Handle = H>>(&mut self, input: Input, provider: &mut P) -> Event {
        if self.editor.is_some() {
            return self.edit(input, provider);
        }
        if input == Input::Back {
            self.message = None;
            return if self.pages.pop().is_some() {
                Event::Changed
            } else {
                Event::Exit
            };
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
                Input::Up => current.saturating_sub(1),
                Input::Down => (current + 1).min(selectable.len().saturating_sub(1)),
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
        if self.pages.is_empty() {
            let set = &self.sets[source].1;
            if let Some(form) = set.forms.first() {
                self.pages.push(Page {
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
        let (handle, set) = self
            .sets
            .iter()
            .find(|s| s.1.guid == page.guid)
            .expect("page formset exists");
        let statement = &set.form(page.form).expect("page form exists").items[source].statement;
        if let Statement::Goto { form, .. } = statement {
            if set.form(*form).is_some() {
                self.pages.push(Page {
                    guid: set.guid,
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
        if matches!(statement, Statement::Action { .. }) {
            let result = provider.action(*handle, set, question);
            return self.result(result);
        }
        let value = match provider.read(*handle, set, question) {
            Ok(value) => value,
            Err(error) => return self.failure(error),
        };
        let kind = match (statement, value) {
            (Statement::Checkbox { .. }, Value::Bool(value)) => {
                let result = provider.write(*handle, set, question, &Value::Bool(!value));
                return self.result(result);
            }
            (Statement::Checkbox { .. }, Value::Number(value)) => {
                let negated = Value::Number(u64::from(value == 0));
                let result = provider.write(*handle, set, question, &negated);
                return self.result(result);
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
            .find(|s| s.1.guid == page.guid)
            .expect("page formset exists");
        let form = set.form(page.form).expect("page form exists");
        form.items
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
                        enabled: !gray && !matches!(kind, RowKind::Heading | RowKind::Info),
                        uncertain: uncertain_hide || uncertain_gray,
                    },
                    help,
                })
            })
            .collect()
    }
    fn failure(&mut self, error: Error) -> Event {
        self.message = Some(error.reason.clone());
        Event::Failed(error)
    }
    fn result(&mut self, result: Result<Response, Error>) -> Event {
        match result {
            Err(error) => self.failure(error),
            Ok(response) => {
                self.message = match &response {
                    Response::Message(text) => Some(text.clone()),
                    Response::Reset => {
                        Some("A reset is required for the change to take effect".into())
                    }
                    _ => None,
                };
                if response == Response::Exit {
                    self.pages.clear();
                }
                Event::Response(response)
            }
        }
    }
    fn edit<P: Provider<Handle = H>>(&mut self, input: Input, provider: &mut P) -> Event {
        if input == Input::Back {
            self.editor = None;
            self.message = None;
            return Event::Changed;
        }
        let (source, editor) = self.editor.as_mut().expect("editor exists");
        if input != Input::Enter {
            match &mut editor.kind {
                EditorKind::Choice { choices, selected } => match input {
                    Input::Up | Input::Left => *selected = selected.saturating_sub(1),
                    Input::Down | Input::Right => {
                        *selected = (*selected + 1).min(choices.len().saturating_sub(1))
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
                            .find(|s| s.1.guid == page.guid)
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
        let (handle, set) = self
            .sets
            .iter()
            .find(|s| s.1.guid == page.guid)
            .expect("editor formset exists");
        let statement = &set.form(page.form).expect("editor form exists").items[*source].statement;
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
        let result = provider.write(
            *handle,
            set,
            statement.question().expect("editor question exists"),
            &value,
        );
        if result.is_ok() {
            self.editor = None;
        }
        self.result(result)
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
    use crate::{Choice, Form, FormId, Item, Question, Storage, Width};
    use alloc::collections::BTreeMap;
    use alloc::{vec, vec::Vec};
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct Memory {
        values: BTreeMap<u16, Value>,
        unreadable: BTreeSet<u16>,
        writes: Vec<(QuestionId, Value)>,
        actions: Vec<QuestionId>,
        responses: BTreeMap<u16, Response>,
    }

    impl Memory {
        fn put(&mut self, id: u16, value: Value) -> &mut Self {
            self.values.insert(id, value);
            self
        }

        fn respond(&mut self, id: u16, response: Response) -> &mut Self {
            self.responses.insert(id, response);
            self
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
            if self.unreadable.contains(&question.id.0) {
                return Err(Error {
                    code: 1,
                    reason: "unreadable".into(),
                });
            }
            Ok(self
                .values
                .get(&question.id.0)
                .cloned()
                .unwrap_or(Value::Number(0)))
        }

        fn write(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
            value: &Value,
        ) -> Result<Response, Error> {
            self.writes.push((question.id, value.clone()));
            self.values.insert(question.id.0, value.clone());
            Ok(self
                .responses
                .get(&question.id.0)
                .cloned()
                .unwrap_or(Response::None))
        }

        fn action(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
        ) -> Result<Response, Error> {
            self.actions.push(question.id);
            Ok(self
                .responses
                .get(&question.id.0)
                .cloned()
                .unwrap_or(Response::None))
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

    fn session() -> Session<u32> {
        Session::new(vec![(7, page())])
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
        let mut session = Session::new(vec![(1, set)]);
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
        assert_eq!(session.view(&mut memory).selected, Some(4));
        session.input(Input::Up, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(2));
        session.input(Input::Home, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(2));
    }

    #[test]
    fn enter_on_action_runs_provider_once() {
        let mut memory = Memory::default();
        memory.respond(5, Response::Message("done".into()));
        let mut session = entered(&mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(2));
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(Response::Message("done".into()))
        );
        assert_eq!(memory.actions, [QuestionId(5)]);
        assert_eq!(session.view(&mut memory).message.as_deref(), Some("done"));
    }

    #[test]
    fn toggle_writes_negation() {
        let mut memory = Memory::default();
        memory
            .put(1, Value::Bool(false))
            .respond(1, Response::Refresh);
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        assert_eq!(session.view(&mut memory).selected, Some(3));
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(Response::Refresh)
        );
        assert_eq!(memory.writes, [(QuestionId(1), Value::Bool(true))]);
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
            Event::Response(Response::None)
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
            Event::Response(Response::None)
        );
        assert_eq!(memory.writes, [(QuestionId(2), Value::Number(20))]);
        assert_eq!(session.view(&mut memory).rows[4].value, "High");
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
        memory.respond(1, Response::Reset);
        let mut session = entered(&mut memory);
        session.input(Input::Down, &mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(Response::Reset)
        );
        let message = session.view(&mut memory).message.unwrap_or_default();
        assert!(message.contains("reset"), "message was {message:?}");

        let mut memory = Memory::default();
        memory.respond(5, Response::Exit);
        let mut session = entered(&mut memory);
        assert_eq!(
            session.input(Input::Enter, &mut memory),
            Event::Response(Response::Exit)
        );
        let view = session.view(&mut memory);
        assert_eq!(view.title, "Formsets");
        assert_eq!(view.rows.len(), 1);
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
        let mut session = Session::new(vec![(1, set)]);
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
        let mut session = Session::new(vec![(1, set)]);
        let mut memory = Memory::default();
        session.input(Input::Enter, &mut memory);
        let view = session.view(&mut memory);
        assert_eq!(view.rows.len(), 1);
        assert!(view.rows[0].uncertain);
        assert!(view.rows[0].enabled);
        assert_eq!(view.selected, Some(0));
    }

    #[test]
    fn read_failure_shows_unavailable() {
        let mut memory = Memory::default();
        memory.unreadable.insert(2);
        let mut session = entered(&mut memory);
        let view = session.view(&mut memory);
        assert_eq!(view.rows[4].value, "<unavailable>");
        assert!(view.rows[4].enabled);
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
        let mut session = Session::new(vec![(1, set.clone())]);
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
}
