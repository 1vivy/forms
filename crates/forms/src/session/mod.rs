//! Navigation, editing, validation and modal state.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec,
    vec::Vec,
};

use crate::ext::{Decoration, EditState, EditStep, Field, Registry};
use crate::modal::{Modal, ModalOutcome};
use crate::model::{
    Control, Error, Expr, FormSet, Key, Presentation, Provider, Refresh, Request, Row, Scope,
    Validation, ValidationKind, Value,
};

/// One front-end input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Select the previous row, choice or button.
    Up,
    /// Select the next row, choice or button.
    Down,
    /// Decrease a value, move an item up, or select the previous choice.
    Left,
    /// Increase a value, move an item down, or select the next choice.
    Right,
    /// Open an editor, accept the selection, or confirm.
    Enter,
    /// Cancel an editor or leave a page.
    Back,
    /// Select the first entry and move the cursor to the start.
    Home,
    /// Select the last entry and move the cursor to the end.
    End,
    /// Remove the character before the cursor.
    Erase,
    /// Insert a character at the cursor.
    Char(char),
}

/// The result of one input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// No state changed.
    None,
    /// The local page, editor or value changed.
    Changed,
    /// A store or provider request ran.
    Response(Request),
    /// A warning asks for confirmation.
    Warning(String),
    /// An operation failed.
    Failed(Error),
    /// The person left the root page.
    Exit,
    /// A modal finished.
    Modal(ModalOutcome),
}

/// One visible row of a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowView {
    /// The row key.
    pub key: Key,
    /// The label.
    pub label: String,
    /// The formatted value.
    pub value: String,
    /// The context help.
    pub help: String,
    /// The drawing of the value.
    pub decoration: Decoration,
    /// The row reacts to Enter.
    pub selectable: bool,
    /// Enter opens another page, so a renderer can mark the row as a link.
    pub link: bool,
    /// The row accepts no change.
    pub enabled: bool,
    /// A condition or a value could not be read.
    pub uncertain: bool,
    /// The presentation hints.
    pub presentation: Presentation,
}

/// The shape of an open editor, for a renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorKind {
    /// A list of choice labels.
    Choice {
        /// The labels in publication order.
        items: Vec<String>,
        /// The highlighted index.
        selected: usize,
    },
    /// A decimal number with a cursor.
    Number {
        /// The entered text.
        text: String,
        /// The cursor position in characters.
        cursor: usize,
    },
    /// A bounded number adjusted with two keys.
    Slider {
        /// The value text, with its unit or its label.
        text: String,
        /// The current position.
        position: u64,
        /// The inclusive lower bound.
        minimum: u64,
        /// The inclusive upper bound.
        maximum: u64,
    },
    /// A text value with a cursor.
    Text {
        /// The entered text.
        text: String,
        /// The cursor position in characters.
        cursor: usize,
        /// Conceal the text.
        secret: bool,
    },
    /// An ordered list.
    List {
        /// The chosen labels, then the available labels, then the closing entry.
        items: Vec<String>,
        /// The cursor index within `items`.
        selected: usize,
        /// The number of chosen entries at the start of `items`.
        chosen: usize,
        /// The selected chosen entry is picked up and moves with Up and Down.
        held: bool,
    },
}

/// An open editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editor {
    /// The row that the editor changes.
    pub row: Key,
    /// The label of the row.
    pub label: String,
    /// The editable state.
    pub kind: EditorKind,
}

/// A renderer-independent snapshot of one page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// Ancestor page titles, starting at the root.
    pub path: Vec<String>,
    /// The current page title.
    pub title: String,
    /// The visible rows in display order.
    pub rows: Vec<RowView>,
    /// The selected index in `rows`.
    pub selected: Option<usize>,
    /// The help of the selected row.
    pub help: String,
    /// The open editor.
    pub editor: Option<Editor>,
    /// A message, refused value, warning or failure.
    pub message: Option<String>,
    /// The open modal.
    pub modal: Option<Modal>,
}

/// One page of the navigation stack.
#[derive(Clone, Copy, Debug)]
struct StackPage<H> {
    handle: H,
    set: Key,
    page: Key,
    selected: Key,
}

/// An open editor.
struct OpenEditor {
    row: Key,
    state: EditState,
    value: Option<Value>,
}

/// The title of the root page.
const ROOT: &str = "Form sets";

/// The prompt of the Back row.
const BACK: &str = "< Back";

/// Navigation, editing and provider state for one front end.
///
/// The session holds no value of its own. It reads a value before it shows it,
/// writes a change through the provider and keeps only navigation state.
///
/// # Examples
///
/// ```
/// use forms::{Builder, Control, Key, MemoryProvider, Session};
///
/// let mut builder = Builder::new(Key(1), "Platform");
/// builder.page("Main");
/// builder.row("Secure boot", Control::Toggle).set_binding(Key(10));
/// let mut provider = MemoryProvider::new(vec![(0u8, builder.build())]);
/// let mut session = Session::new(provider.sets.clone());
/// assert!(session.enter(Key(1)));
/// let view = session.view(&mut provider);
/// assert_eq!(view.rows[0].label, "Secure boot");
/// ```
pub struct Session<H> {
    sets: Vec<(H, FormSet)>,
    pages: Vec<StackPage<H>>,
    root: Option<usize>,
    editor: Option<OpenEditor>,
    message: Option<String>,
    /// Pages at the bottom of the stack that Back does not pop.
    base: usize,
    back_row: bool,
    entry_back_row: bool,
    readable_rows: bool,
    enter_cancels_unchanged: bool,
    modal: Option<Modal>,
    registry: Registry,
    /// The last read time of each value key.
    reads: BTreeMap<Key, u64>,
    /// The event keys that the consumer signalled.
    signals: BTreeSet<Key>,
    /// The row whose warning the person confirmed with the last input.
    confirmed: Option<Key>,
    now: u64,
}

impl<H: Copy + Eq> Session<H> {
    /// Creates a session over published sets, sorted by title.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new());
    /// ```
    pub fn new(mut sets: Vec<(H, FormSet)>) -> Self {
        sets.sort_by(|left, right| left.1.title.cmp(&right.1.title));
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
            enter_cancels_unchanged: false,
            modal: None,
            registry: Registry::builtin(),
            reads: BTreeMap::new(),
            signals: BTreeSet::new(),
            confirmed: None,
            now: 0,
        }
    }

    /// Chooses whether pages end with a Back row. The default is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new()).with_back_row(false);
    /// ```
    #[must_use]
    pub fn with_back_row(mut self, back_row: bool) -> Self {
        self.back_row = back_row;
        self
    }

    /// Chooses whether the pinned entry page has a Back row. The default is
    /// `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new()).with_entry_back_row(false);
    /// ```
    #[must_use]
    pub fn with_entry_back_row(mut self, entry_back_row: bool) -> Self {
        self.entry_back_row = entry_back_row;
        self
    }

    /// Chooses whether information rows can be selected. The default is
    /// `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new()).with_readable_rows(true);
    /// ```
    #[must_use]
    pub fn with_readable_rows(mut self, readable_rows: bool) -> Self {
        self.readable_rows = readable_rows;
        self
    }

    /// Chooses whether Enter on an unchanged value closes an editor without a
    /// write. The default is `false`.
    ///
    /// A device without a Back key has no other way to leave an editor
    /// unchanged. The option covers value editors (choice, number, slider,
    /// text, ordered list); an editor whose value the person moved still
    /// commits on Enter.
    ///
    /// # Examples
    ///
    /// ```
    /// let _session = forms::Session::<u32>::new(Vec::new()).with_enter_cancels_unchanged(true);
    /// ```
    #[must_use]
    pub fn with_enter_cancels_unchanged(mut self, enter_cancels_unchanged: bool) -> Self {
        self.enter_cancels_unchanged = enter_cancels_unchanged;
        self
    }

    /// Uses a registry with consumer handlers.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Registry, Session};
    ///
    /// let session = Session::<u32>::new(Vec::new())
    ///     .with_registry(Registry::builtin());
    /// ```
    #[must_use]
    pub fn with_registry(mut self, registry: Registry) -> Self {
        self.registry = registry;
        self
    }

    /// Shows a modal over the current page.
    ///
    /// The modal takes every input until it closes. The page, its selection
    /// and an open editor stay unchanged underneath it. A second call replaces
    /// an open modal.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Modal, ModalStyle, Session};
    ///
    /// let mut session = Session::<u32>::new(Vec::new());
    /// session.open_modal(Modal::new(ModalStyle::Info, "Notice", "The write finished."));
    /// ```
    pub fn open_modal(&mut self, modal: Modal) {
        self.modal = Some(modal);
    }

    /// Opens the entry page of a set and pins it.
    ///
    /// Returns `false` and changes nothing when no set has this key or the set
    /// has no page.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut session = forms::Session::<u32>::new(Vec::new());
    /// assert!(!session.enter(forms::Key(1)));
    /// ```
    pub fn enter(&mut self, set_key: Key) -> bool {
        let Some((handle, page)) = self
            .sets
            .iter()
            .find(|(_, set)| set.key == set_key)
            .and_then(|(handle, set)| set.pages.first().map(|page| (*handle, page.key)))
        else {
            return false;
        };
        self.pages = vec![StackPage {
            handle,
            set: set_key,
            page,
            selected: Key::default(),
        }];
        self.base = 1;
        self.editor = None;
        self.message = None;
        true
    }

    /// Replaces the published sets and keeps the pages that still exist.
    ///
    /// An open editor is cancelled because its row may have changed.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut session = forms::Session::<u32>::new(Vec::new());
    /// session.replace(Vec::new());
    /// ```
    pub fn replace(&mut self, mut sets: Vec<(H, FormSet)>) {
        let root = self
            .root
            .and_then(|index| self.sets.get(index))
            .map(|(handle, set)| (*handle, set.key));
        sets.sort_by(|left, right| left.1.title.cmp(&right.1.title));
        self.root = root.and_then(|(handle, set)| {
            sets.iter()
                .position(|(other, candidate)| *other == handle && candidate.key == set)
        });
        let valid = self
            .pages
            .iter()
            .take_while(|page| {
                sets.iter().any(|(handle, set)| {
                    *handle == page.handle && set.key == page.set && set.page(page.page).is_some()
                })
            })
            .count();
        self.pages.truncate(valid);
        self.base = self.base.min(self.pages.len());
        self.sets = sets;
        self.editor = None;
    }

    /// Records an event key from the consumer.
    ///
    /// Returns `true` when a visible row refreshes on that key, so the caller
    /// draws the page again.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut session = forms::Session::<u32>::new(Vec::new());
    /// assert!(!session.signal(forms::Key(5)));
    /// ```
    pub fn signal(&mut self, key: Key) -> bool {
        self.signals.insert(key);
        self.current().is_some_and(|(_, set, page)| {
            set.page(page).is_some_and(|page| {
                page.rows
                    .iter()
                    .any(|row| row.refresh == Refresh::Event(key))
            })
        })
    }

    /// Advances time and reports whether a periodic refresh is due.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut session = forms::Session::<u32>::new(Vec::new());
    /// assert!(!session.poll(10_000));
    /// ```
    pub fn poll(&mut self, now: u64) -> bool {
        self.now = now;
        let Some((_, set, page)) = self.current() else {
            return false;
        };
        let Some(page) = set.page(page) else {
            return false;
        };
        page.rows.iter().any(|row| match row.refresh {
            Refresh::Periodic(interval) => {
                let Some(binding) = row.binding else {
                    return false;
                };
                match self.reads.get(&binding) {
                    Some(last) => now.saturating_sub(*last) >= interval,
                    None => true,
                }
            }
            Refresh::OnView | Refresh::Event(_) => false,
        })
    }

    /// Reads a fresh snapshot of the current page.
    ///
    /// Values come from the provider. A failed read leaves the value empty and
    /// marks the row uncertain; it does not abort the snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut provider = forms::MemoryProvider::new(Vec::new());
    /// let mut session = forms::Session::<u8>::new(Vec::new());
    /// assert_eq!(session.view(&mut provider).title, "Form sets");
    /// ```
    pub fn view<P: Provider<Handle = H>>(&mut self, provider: &mut P) -> View {
        let Some((handle, set, page_key)) = self.current() else {
            return self.root_view();
        };
        let editing = self
            .editor
            .as_ref()
            .map(|editor| (editor.row, editor.state.clone()));
        let mut rows = Vec::new();
        let mut selected_key = Key::default();
        let mut any_selected = false;
        if let Some(page) = set.page(page_key) {
            selected_key = self
                .pages
                .last()
                .map_or(Key::default(), |page| page.selected);
            for row in &page.rows {
                let mut uncertain = false;
                match self.condition(provider, handle, &set, row.hidden.as_ref()) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(_) => uncertain = true,
                }
                let disabled = match self.condition(provider, handle, &set, row.disabled.as_ref()) {
                    Ok(disabled) => disabled,
                    Err(_) => {
                        uncertain = true;
                        false
                    }
                };
                let locked = match self.read_only(provider, handle, &set, row) {
                    Ok(read_only) => read_only,
                    Err(_) => {
                        uncertain = true;
                        false
                    }
                };
                let value = match row.binding {
                    Some(binding) => match self.read_value(provider, handle, &set, row) {
                        Ok(value) => {
                            self.reads.insert(binding, self.now);
                            Some(value)
                        }
                        Err(_) => {
                            uncertain = true;
                            None
                        }
                    },
                    None => None,
                };
                let field = Field {
                    row,
                    value: value.as_ref(),
                };
                let live = editing
                    .as_ref()
                    .filter(|(key, _)| *key == row.key)
                    .and_then(|(_, state)| {
                        self.registry
                            .handler(&row.control)
                            .and_then(|handler| handler.format_editing(&field, state))
                    });
                let (text, decoration) =
                    live.unwrap_or_else(|| self.registry.format(row, value.as_ref()));
                let selectable = row.control.is_interactive()
                    && !disabled
                    && (self.readable_rows || !matches!(row.control, Control::Text(_)));
                if selectable && !any_selected && selected_key == Key::default() {
                    selected_key = row.key;
                    any_selected = true;
                }
                rows.push(RowView {
                    key: row.key,
                    label: row.label.clone(),
                    value: text,
                    help: row.help.clone(),
                    decoration,
                    selectable,
                    link: matches!(row.control, Control::Link(_)),
                    enabled: !disabled && !locked,
                    uncertain,
                    presentation: row.presentation,
                });
            }
        }
        if self.back_row {
            rows.push(back_row());
        }
        let position = rows.iter().position(|row| row.key == selected_key);
        let selected = position.or_else(|| rows.iter().position(|row| row.selectable));
        if let (Some(index), Some(page)) = (selected, self.pages.last_mut()) {
            page.selected = rows[index].key;
        }
        let help = selected.map_or_else(String::new, |index| rows[index].help.clone());
        let mut path = Vec::new();
        if self.base == 0 && !self.pages.is_empty() {
            path.push(String::from(ROOT));
        }
        let ancestors = self.pages.len().saturating_sub(1);
        for page in self.pages.iter().take(ancestors).skip(self.base) {
            let title = self
                .sets
                .iter()
                .find(|(handle, set)| *handle == page.handle && set.key == page.set)
                .and_then(|(_, set)| set.page(page.page))
                .map(|page| page.title.clone());
            if let Some(title) = title {
                path.push(title);
            }
        }
        let title = set
            .page(page_key)
            .map_or_else(String::new, |page| page.title.clone());
        View {
            path,
            title,
            rows,
            selected,
            help,
            editor: self.editor_view(&set),
            message: self.message.clone(),
            modal: self.modal.clone(),
        }
    }

    /// Applies one input.
    ///
    /// An invalid value keeps the editor open, sets the message and returns
    /// [`Event::Failed`] without a provider write. A warning returns
    /// [`Event::Warning`] and needs a second Enter to confirm.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms::{Event, Input, Session};
    ///
    /// let mut provider = forms::MemoryProvider::new(Vec::new());
    /// let mut session = forms::Session::<u8>::new(Vec::new());
    /// assert_eq!(session.input(Input::Enter, &mut provider), Event::None);
    /// ```
    pub fn input<P: Provider<Handle = H>>(&mut self, input: Input, provider: &mut P) -> Event {
        if let Some(mut modal) = self.modal.take() {
            match modal.input(input) {
                Some(outcome) => return Event::Modal(outcome),
                None => {
                    self.modal = Some(modal);
                    return Event::Changed;
                }
            }
        }
        if self.editor.is_some() {
            return self.edit(input, provider);
        }
        match input {
            Input::Up | Input::Down | Input::Home | Input::End => {
                self.move_selection(input, provider)
            }
            Input::Enter => self.activate(provider),
            Input::Back => self.back(provider),
            Input::Char(_) | Input::Erase | Input::Left | Input::Right => Event::None,
        }
    }

    /// Moves the selection within the visible rows.
    fn move_selection<P: Provider<Handle = H>>(&mut self, input: Input, provider: &mut P) -> Event {
        let Some((handle, set, page_key)) = self.current() else {
            let count = self.sets.len();
            if count == 0 {
                return Event::None;
            }
            let index = self.root.unwrap_or(0);
            let next = match input {
                Input::Up => (index + count - 1) % count,
                Input::Down => (index + 1) % count,
                Input::Home => 0,
                _ => count - 1,
            };
            self.root = Some(next);
            return Event::Changed;
        };
        let Some(page) = set.page(page_key) else {
            return Event::None;
        };
        let mut visible: Vec<Key> = Vec::new();
        for row in &page.rows {
            if !(row.control.is_interactive() || self.readable_rows) {
                continue;
            }
            match self.condition(provider, handle, &set, row.hidden.as_ref()) {
                Ok(true) | Err(_) => continue,
                Ok(false) => visible.push(row.key),
            }
        }
        // The view ends every page with the Back row, so Up and Down reach it:
        // a device with only volume keys and power has no other way back.
        if self.back_row {
            visible.push(back_row().key);
        }
        if visible.is_empty() {
            return Event::None;
        }
        let selected = self
            .pages
            .last()
            .map_or(Key::default(), |page| page.selected);
        let index = visible.iter().position(|key| *key == selected).unwrap_or(0);
        let last = visible.len() - 1;
        let next = match input {
            Input::Up => {
                if index == 0 {
                    last
                } else {
                    index - 1
                }
            }
            Input::Down => {
                if index == last {
                    0
                } else {
                    index + 1
                }
            }
            Input::Home => 0,
            _ => last,
        };
        if let Some(page) = self.pages.last_mut() {
            page.selected = visible[next];
        }
        self.message = None;
        Event::Changed
    }

    /// Runs the selected row.
    fn activate<P: Provider<Handle = H>>(&mut self, provider: &mut P) -> Event {
        let Some((handle, set, page_key)) = self.current() else {
            let Some(index) = self
                .root
                .or(if self.sets.is_empty() { None } else { Some(0) })
            else {
                return Event::None;
            };
            let Some((handle, set)) = self.sets.get(index) else {
                return Event::None;
            };
            let Some(page) = set.pages.first().map(|page| page.key) else {
                return Event::None;
            };
            let handle = *handle;
            let set_key = set.key;
            self.pages = vec![StackPage {
                handle,
                set: set_key,
                page,
                selected: Key::default(),
            }];
            self.root = Some(index);
            return Event::Changed;
        };
        let Some(page) = set.page(page_key) else {
            return Event::None;
        };
        let selected = self
            .pages
            .last()
            .map_or(Key::default(), |page| page.selected);
        let row = page.rows.iter().find(|row| row.key == selected).cloned();
        let row = match row {
            Some(row) => Some(row),
            None => {
                let mut first = None;
                for candidate in &page.rows {
                    if !(candidate.control.is_interactive() || self.readable_rows) {
                        continue;
                    }
                    match self.condition(provider, handle, &set, candidate.hidden.as_ref()) {
                        Ok(true) | Err(_) => {}
                        Ok(false) => {
                            first = Some(candidate.clone());
                            break;
                        }
                    }
                }
                first
            }
        };
        let Some(row) = row else {
            if self.back_row {
                return self.back(provider);
            }
            return Event::None;
        };
        if let Some(page) = self.pages.last_mut() {
            page.selected = row.key;
        }
        match self.condition(provider, handle, &set, row.disabled.as_ref()) {
            Ok(true) => {
                let error = Error::new("The row is disabled");
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
            Ok(false) => {}
            Err(error) => {
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
        }
        match self.read_only(provider, handle, &set, &row) {
            Ok(true) => {
                let error = Error::new("The value is read-only");
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
            Ok(false) => {}
            Err(error) => {
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
        }
        let opens = self
            .registry
            .handler(&row.control)
            .is_some_and(|handler| handler.opens_editor());
        let value = match row.binding {
            Some(_) => match self.read_value(provider, handle, &set, &row) {
                Ok(value) => Some(value),
                Err(error) => {
                    self.message = Some(error.reason.clone());
                    return Event::Failed(error);
                }
            },
            None => None,
        };
        if opens {
            let field = Field {
                row: &row,
                value: value.as_ref(),
            };
            let state = self
                .registry
                .handler(&row.control)
                .map_or_else(EditState::default, |handler| handler.start(&field));
            self.editor = Some(OpenEditor {
                row: row.key,
                state,
                value,
            });
            self.message = None;
            return Event::Changed;
        }
        let mut state = EditState::default();
        let step = self
            .registry
            .edit(&row, value.as_ref(), &mut state, Input::Enter);
        self.apply_step(provider, handle, set, row, step)
    }

    /// Applies one editor input.
    fn edit<P: Provider<Handle = H>>(&mut self, input: Input, provider: &mut P) -> Event {
        if input == Input::Back {
            self.editor = None;
            self.message = None;
            return Event::Changed;
        }
        let Some((handle, set, _)) = self.current() else {
            self.editor = None;
            return Event::Changed;
        };
        let Some(open) = self.editor.take() else {
            return Event::Changed;
        };
        let Some(row) = set.row(open.row).cloned() else {
            return Event::Changed;
        };
        let mut state = open.state;
        let step = self
            .registry
            .edit(&row, open.value.as_ref(), &mut state, input);
        match step {
            // An unchanged value leaves the editor without a write, so a device
            // with no Back key can still back out of an editor.
            EditStep::Accept(value)
                if self.enter_cancels_unchanged && open.value.as_ref() == Some(&value) =>
            {
                self.message = None;
                Event::Changed
            }
            EditStep::Accept(value) => {
                self.editor = Some(OpenEditor { state, ..open });
                self.commit(provider, handle, set, row, value)
            }
            other => {
                self.editor = Some(OpenEditor {
                    row: open.row,
                    state,
                    value: open.value,
                });
                self.apply_step(provider, handle, set, row, other)
            }
        }
    }

    /// Applies a non-accepting edit step.
    fn apply_step<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: FormSet,
        row: Row,
        step: EditStep,
    ) -> Event {
        match step {
            EditStep::Ignored => Event::None,
            EditStep::Changed => {
                self.message = None;
                Event::Changed
            }
            EditStep::Refused(reason) => {
                self.message = Some(reason.clone());
                Event::Failed(Error::new(reason))
            }
            EditStep::Accept(value) => self.commit(provider, handle, set, row, value),
            EditStep::Invoke => self.invoke(provider, handle, set, row),
        }
    }

    /// Runs the command of a row that has no value editor.
    fn invoke<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: FormSet,
        row: Row,
    ) -> Event {
        self.editor = None;
        if let Some(default) = row.control.reset_default() {
            return self.reset(provider, handle, &set, default);
        }
        if let Some(target) = row.control.target()
            && (target.set.is_none() || target.set == Some(set.key))
        {
            if let Err(error) = self.check_submit(provider, handle, &set) {
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
            // A link opens its page on top of the current one, so Back returns
            // to the row that linked, as a browser does with a goto.
            if let Some(current) = self.pages.last().copied() {
                self.pages.push(StackPage {
                    page: target.page,
                    selected: target.row.unwrap_or_default(),
                    ..current
                });
            }
            self.message = None;
            return Event::Changed;
        }
        match provider.action(handle, &set, &row) {
            Ok(request) => {
                self.message = None;
                self.run_request(provider, handle, &set, request)
            }
            Err(error) => {
                self.message = Some(error.reason.clone());
                Event::Failed(error)
            }
        }
    }

    /// Checks the edit rules and writes an accepted value.
    ///
    /// An edit rule refuses the value. A warning rule asks for one more
    /// Enter and lets the confirmed input through.
    fn commit<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: FormSet,
        row: Row,
        value: Value,
    ) -> Event {
        match self.check(provider, handle, &set, &row, ValidationKind::Edit) {
            Ok(Some(rule)) => {
                self.confirmed = None;
                self.message = Some(rule.message.clone());
                return Event::Failed(Error::new(rule.message));
            }
            Ok(None) => {}
            Err(error) => {
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
        }
        let confirmed = self.confirmed == Some(row.key);
        match self.check(provider, handle, &set, &row, ValidationKind::Warning) {
            Ok(Some(rule)) if !confirmed => {
                self.confirmed = Some(row.key);
                self.message = Some(rule.message.clone());
                return Event::Warning(rule.message);
            }
            Ok(_) => {}
            Err(error) => {
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
        }
        match provider.write(handle, &set, &row, &value) {
            Ok(request) => {
                self.confirmed = None;
                self.editor = None;
                self.message = None;
                self.run_request(provider, handle, &set, request)
            }
            Err(error) => {
                self.message = Some(error.reason.clone());
                Event::Failed(error)
            }
        }
    }

    /// Writes the values of one default set.
    fn reset<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
        default: Key,
    ) -> Event {
        let Some(defaults) = set.defaults(default) else {
            self.message = Some(String::from("The default set is missing"));
            return Event::Failed(Error::new("The default set is missing"));
        };
        for (binding, value) in &defaults.values {
            let Some(row) = set
                .pages
                .iter()
                .flat_map(|page| &page.rows)
                .find(|row| row.binding == Some(*binding))
            else {
                continue;
            };
            if let Err(error) = provider.write(handle, set, row, value) {
                self.message = Some(error.reason.clone());
                return Event::Failed(error);
            }
        }
        match provider.apply(handle, set, Scope::Set) {
            Ok(()) => {
                self.message = None;
                self.editor = None;
                Event::Changed
            }
            Err(error) => {
                self.message = Some(error.reason.clone());
                Event::Failed(error)
            }
        }
    }

    /// Runs the request that a provider returned.
    fn run_request<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
        request: Request,
    ) -> Event {
        match request {
            Request::None | Request::Restart => {}
            Request::Apply { scope, leave } => {
                if let Err(error) = provider.apply(handle, set, scope) {
                    self.message = Some(error.reason.clone());
                    return Event::Failed(error);
                }
                if leave {
                    self.leave(scope);
                }
            }
            Request::Discard { scope, leave } => {
                if let Err(error) = provider.discard(handle, set, scope) {
                    self.message = Some(error.reason.clone());
                    return Event::Failed(error);
                }
                if leave {
                    self.leave(scope);
                }
            }
            Request::Reconnect => {
                if let Err(error) = provider.reconnect(handle) {
                    self.message = Some(error.reason.clone());
                    return Event::Failed(error);
                }
            }
        }
        Event::Response(request)
    }

    /// Leaves a page after a request that asks for it.
    fn leave(&mut self, scope: Scope) {
        let Some(page) = self.pages.last() else {
            return;
        };
        let leaves = match scope {
            Scope::Page(key) => key == page.page,
            Scope::Set => true,
            Scope::Row(_) => false,
        };
        if leaves && self.pages.len() > self.base {
            self.pages.pop();
        }
    }

    /// Leaves the current page or the session.
    fn back<P: Provider<Handle = H>>(&mut self, provider: &mut P) -> Event {
        if self.editor.take().is_some() {
            return Event::Changed;
        }
        let Some((handle, set, _)) = self.current() else {
            return Event::Exit;
        };
        if let Err(error) = self.check_submit(provider, handle, &set) {
            self.message = Some(error.reason.clone());
            return Event::Failed(error);
        }
        if self.pages.len() > self.base {
            self.pages.pop();
            self.message = None;
            return Event::Changed;
        }
        if self.pages.is_empty() {
            return Event::Exit;
        }
        Event::Exit
    }

    /// Checks the submit rules of the active page.
    fn check_submit<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
    ) -> Result<(), Error> {
        let Some(page) = self.pages.last().and_then(|page| set.page(page.page)) else {
            return Ok(());
        };
        for row in &page.rows {
            if let Some(rule) = self.check(provider, handle, set, row, ValidationKind::Submit)? {
                return Err(Error::new(rule.message));
            }
        }
        Ok(())
    }

    /// Returns the first true rule of one kind for a row.
    fn check<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
        row: &Row,
        kind: ValidationKind,
    ) -> Result<Option<Validation>, Error> {
        for rule in &row.validation {
            if rule.kind != kind {
                continue;
            }
            if self.evaluate(provider, handle, set, &rule.condition)? {
                return Ok(Some(rule.clone()));
            }
        }
        Ok(None)
    }

    /// Evaluates an expression against provider values.
    fn evaluate<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
        expr: &Expr,
    ) -> Result<bool, Error> {
        expr.evaluate(&mut |key| {
            let row = set
                .pages
                .iter()
                .flat_map(|page| &page.rows)
                .find(|row| row.binding == Some(key))
                .ok_or_else(|| Error::new("The expression names an unknown value"))?;
            provider.read(handle, set, row)
        })
    }

    /// Evaluates an optional condition. An absent condition is false.
    fn condition<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
        expr: Option<&Expr>,
    ) -> Result<bool, Error> {
        match expr {
            Some(expr) => self.evaluate(provider, handle, set, expr),
            None => Ok(false),
        }
    }

    /// Returns whether a row is read-only.
    fn read_only<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
        row: &Row,
    ) -> Result<bool, Error> {
        if row.locked {
            return Ok(true);
        }
        self.condition(provider, handle, set, row.read_only.as_ref())
    }

    /// Reads the value of a bound row.
    fn read_value<P: Provider<Handle = H>>(
        &mut self,
        provider: &mut P,
        handle: H,
        set: &FormSet,
        row: &Row,
    ) -> Result<Value, Error> {
        if row.binding.is_none() {
            return Err(Error::new("The row has no value"));
        }
        provider.read(handle, set, row)
    }

    /// Returns the handle, set and page of the active page.
    fn current(&self) -> Option<(H, FormSet, Key)> {
        let page = self.pages.last()?;
        let (_, set) = self
            .sets
            .iter()
            .find(|(handle, set)| *handle == page.handle && set.key == page.set)?;
        Some((page.handle, set.clone(), page.page))
    }

    /// Builds the editor view of the open editor.
    fn editor_view(&self, set: &FormSet) -> Option<Editor> {
        let open = self.editor.as_ref()?;
        let row = set.row(open.row)?;
        let field = Field {
            row,
            value: self.editor.as_ref().and_then(|open| open.value.as_ref()),
        };
        self.registry
            .handler(&row.control)?
            .editor(&field, &open.state)
    }

    /// Builds the root page of sets.
    fn root_view(&self) -> View {
        let rows: Vec<RowView> = self
            .sets
            .iter()
            .map(|(_, set)| RowView {
                key: set.key,
                label: set.title.clone(),
                value: String::new(),
                help: set.help.clone(),
                decoration: Decoration::Plain,
                selectable: !set.pages.is_empty(),
                link: !set.pages.is_empty(),
                enabled: true,
                uncertain: false,
                presentation: Presentation::default(),
            })
            .collect();
        let selected = self
            .root
            .filter(|index| *index < rows.len())
            .or(if rows.is_empty() { None } else { Some(0) });
        let help = selected.map_or_else(String::new, |index| rows[index].help.clone());
        View {
            path: Vec::new(),
            title: String::from(ROOT),
            rows,
            selected,
            help,
            editor: None,
            message: self.message.clone(),
            modal: self.modal.clone(),
        }
    }
}

/// Builds the Back row.
fn back_row() -> RowView {
    RowView {
        key: Key(u64::MAX),
        label: String::from(BACK),
        value: String::new(),
        help: String::new(),
        decoration: Decoration::Plain,
        selectable: true,
        link: false,
        enabled: true,
        uncertain: false,
        presentation: Presentation::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{BACK, Editor, EditorKind, Event, Input, Session};
    use crate::model::{
        Builder, Choice, Control, Expr, FormSet, Key, MemoryProvider, Request, Store, Validation,
        Value,
    };
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Builds a provider over one set.
    fn provider_of(set: &FormSet) -> MemoryProvider {
        MemoryProvider::new(vec![(0u8, set.clone())])
    }

    /// Builds a session over one set without a Back row.
    fn session_of(set: &FormSet) -> Session<u8> {
        Session::new(vec![(0u8, set.clone())]).with_back_row(false)
    }

    /// Refuses a number outside its range and keeps the stored value.
    #[test]
    fn number_range_refuses_the_value() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Count",
                Control::Number {
                    minimum: 0,
                    maximum: 10,
                    step: 1,
                },
            )
            .set_binding(Key(11));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(11), Value::Number(5));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
        assert_eq!(
            session.input(Input::Char('9'), &mut provider),
            Event::Changed
        );
        assert!(matches!(
            session.input(Input::Enter, &mut provider),
            Event::Failed(_)
        ));
        assert_eq!(provider.store.read(Key(11)), Ok(Value::Number(5)));
    }

    /// Refuses a value that an edit rule rejects and keeps the stored value.
    #[test]
    fn edit_rule_refuses_the_value() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Name",
                Control::TextInput {
                    minimum: 0,
                    maximum: 8,
                    secret: false,
                },
            )
            .set_binding(Key(20))
            .push_validation(Validation::refuse(Expr::Bool(true), "Refused"));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(20), Value::Text(String::from("ok")));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        session.input(Input::Enter, &mut provider);
        session.input(Input::Char('!'), &mut provider);
        assert!(matches!(
            session.input(Input::Enter, &mut provider),
            Event::Failed(_)
        ));
        assert_eq!(
            provider.store.read(Key(20)),
            Ok(Value::Text(String::from("ok")))
        );
    }

    /// Asks for one confirmation before a warned value is written.
    #[test]
    fn warning_needs_a_second_enter() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row("Mode", Control::Toggle)
            .set_binding(Key(21))
            .push_validation(Validation::warn(Expr::Bool(true), "Careful"));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(21), Value::Bool(false));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        assert!(matches!(
            session.input(Input::Enter, &mut provider),
            Event::Warning(_)
        ));
        assert_eq!(provider.store.read(Key(21)), Ok(Value::Bool(false)));
        assert_eq!(
            session.input(Input::Enter, &mut provider),
            Event::Response(Request::None)
        );
        assert_eq!(provider.store.read(Key(21)), Ok(Value::Bool(true)));
    }

    /// Moves an entry of an ordered list and writes the new order.
    #[test]
    fn ordered_list_moves_and_commits() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Order",
                Control::OrderedList {
                    choices: vec![
                        Choice::new("A", 1),
                        Choice::new("B", 2),
                        Choice::new("C", 3),
                    ],
                    maximum: 3,
                },
            )
            .set_binding(Key(12));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(12), Value::List(vec![1, 2]));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
        assert_eq!(session.input(Input::Down, &mut provider), Event::Changed);
        assert_eq!(session.input(Input::Left, &mut provider), Event::Changed);
        assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
        assert_eq!(
            session.input(Input::Enter, &mut provider),
            Event::Response(Request::None)
        );
        assert_eq!(provider.store.read(Key(12)), Ok(Value::List(vec![2, 1])));
    }

    /// Reorders and removes ordered-list entries with Up, Down and Enter only,
    /// as a device with volume keys and power does.
    #[test]
    fn ordered_list_reorders_with_three_keys() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Order",
                Control::OrderedList {
                    choices: vec![
                        Choice::new("A", 1),
                        Choice::new("B", 2),
                        Choice::new("C", 3),
                    ],
                    maximum: 3,
                },
            )
            .set_binding(Key(12));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(12), Value::List(vec![1, 2, 3]));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        // Open the editor, pick up A, move it down one, drop it: B A C.
        for input in [Input::Enter, Input::Enter, Input::Down, Input::Enter] {
            assert_eq!(session.input(input, &mut provider), Event::Changed);
        }
        // Pick up C and move it past the last chosen entry: removed.
        for input in [Input::Down, Input::Enter, Input::Down] {
            assert_eq!(session.input(input, &mut provider), Event::Changed);
        }
        // Walk to Done and accept.
        let Some(Editor {
            kind: EditorKind::List {
                items, selected, ..
            },
            ..
        }) = session.view(&mut provider).editor
        else {
            panic!("expected the list editor");
        };
        for _ in selected..items.len() - 1 {
            session.input(Input::Down, &mut provider);
        }
        assert_eq!(
            session.input(Input::Enter, &mut provider),
            Event::Response(Request::None)
        );
        assert_eq!(provider.store.read(Key(12)), Ok(Value::List(vec![2, 1])));
    }

    /// With the option on, Enter on an unchanged value leaves the editor
    /// without a write; with it off, the same Enter writes the value.
    #[test]
    fn enter_on_an_unchanged_value_cancels_only_when_chosen() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Mode",
                Control::Choice(vec![Choice::new("A", 1), Choice::new("B", 2)]),
            )
            .set_binding(Key(14));
        let set = builder.build();
        for (cancels, expected) in [
            (true, Event::Changed),
            (false, Event::Response(Request::None)),
        ] {
            let mut provider = provider_of(&set);
            provider.set(Key(14), Value::Number(1));
            let mut session = session_of(&set).with_enter_cancels_unchanged(cancels);
            assert!(session.enter(Key(1)));
            session.input(Input::Enter, &mut provider);
            assert_eq!(
                session.input(Input::Enter, &mut provider),
                expected,
                "cancels={cancels}"
            );
            assert!(session.view(&mut provider).editor.is_none());
        }
    }

    /// Down moves the choice cursor without a write; Enter writes the option
    /// under the cursor.
    #[test]
    fn a_choice_writes_only_on_enter() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Mode",
                Control::Choice(vec![Choice::new("A", 1), Choice::new("B", 2)]),
            )
            .set_binding(Key(14));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(14), Value::Number(1));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        session.input(Input::Enter, &mut provider);
        assert_eq!(session.input(Input::Down, &mut provider), Event::Changed);
        assert_eq!(provider.store.read(Key(14)), Ok(Value::Number(1)));
        assert_eq!(
            session.input(Input::Enter, &mut provider),
            Event::Response(Request::None)
        );
        assert_eq!(provider.store.read(Key(14)), Ok(Value::Number(2)));
    }

    /// A link pushes its page; Back returns to the linking page with the link
    /// row still selected.
    #[test]
    fn a_link_nests_and_back_returns_to_the_linking_row() {
        let mut builder = Builder::new(Key(1), "S");
        let inner = builder.key();
        builder.page("Main");
        builder.row("A", Control::Toggle).set_binding(Key(30));
        let link = builder.link("Inner", inner).key;
        builder.page_with(inner, "Inner");
        builder.row("B", Control::Toggle).set_binding(Key(31));
        let set = builder.build();
        let mut provider = provider_of(&set);
        let mut session = Session::new(vec![(0u8, set.clone())]);
        assert!(session.enter(Key(1)));
        session.input(Input::Down, &mut provider);
        assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
        assert_eq!(session.view(&mut provider).title, "Inner");
        assert_eq!(session.input(Input::Back, &mut provider), Event::Changed);
        let view = session.view(&mut provider);
        assert_eq!(view.title, "Main");
        assert_eq!(view.selected.map(|index| view.rows[index].key), Some(link));
    }

    /// Steps a number with Up and Down within its bounds.
    #[test]
    fn number_steps_with_up_and_down() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Count",
                Control::Number {
                    minimum: 0,
                    maximum: 10,
                    step: 5,
                },
            )
            .set_binding(Key(13));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(13), Value::Number(5));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        for input in [Input::Enter, Input::Up, Input::Up] {
            session.input(input, &mut provider);
        }
        assert_eq!(
            session.input(Input::Enter, &mut provider),
            Event::Response(Request::None)
        );
        assert_eq!(provider.store.read(Key(13)), Ok(Value::Number(10)));
    }

    /// Down from the last row reaches the Back row, so a device without a Back
    /// key can always leave the page.
    #[test]
    fn down_reaches_the_back_row() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder.row("A", Control::Toggle).set_binding(Key(30));
        builder.row("B", Control::Toggle).set_binding(Key(31));
        let set = builder.build();
        let mut provider = provider_of(&set);
        let mut session = Session::new(vec![(0u8, set.clone())]);
        assert!(session.enter(Key(1)));
        let mut labels = Vec::new();
        for _ in 0..3 {
            session.input(Input::Down, &mut provider);
            let view = session.view(&mut provider);
            labels.push(view.selected.map(|index| view.rows[index].label.clone()));
        }
        assert_eq!(
            labels,
            [
                Some(String::from("B")),
                Some(String::from(BACK)),
                Some(String::from("A"))
            ]
        );
    }

    /// Writes every value of the named default set.
    #[test]
    fn reset_writes_the_default_set() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder.row("A", Control::Toggle).set_binding(Key(30));
        builder.row("B", Control::Toggle).set_binding(Key(31));
        builder.row("Restore", Control::Reset(Key(40)));
        builder.defaults(
            Key(40),
            "Factory",
            [(Key(30), Value::Bool(false)), (Key(31), Value::Bool(false))],
        );
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(30), Value::Bool(true));
        provider.set(Key(31), Value::Bool(true));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        session.input(Input::Down, &mut provider);
        session.input(Input::Down, &mut provider);
        assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
        assert_eq!(provider.store.read(Key(30)), Ok(Value::Bool(false)));
        assert_eq!(provider.store.read(Key(31)), Ok(Value::Bool(false)));
    }

    /// Hides a row and refuses a read-only row.
    #[test]
    fn conditions_hide_and_lock_rows() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder.row("Kept", Control::Toggle).set_binding(Key(50));
        builder
            .row("Hidden", Control::Toggle)
            .set_binding(Key(51))
            .hidden = Some(Expr::Bool(true));
        builder
            .row("Fixed", Control::Toggle)
            .set_binding(Key(52))
            .read_only = Some(Expr::Bool(true));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(50), Value::Bool(false));
        provider.set(Key(51), Value::Bool(false));
        provider.set(Key(52), Value::Bool(false));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        let view = session.view(&mut provider);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[1].label, "Fixed");
        assert!(!view.rows[1].enabled);
        session.input(Input::Down, &mut provider);
        assert!(matches!(
            session.input(Input::Enter, &mut provider),
            Event::Failed(_)
        ));
        assert_eq!(provider.store.read(Key(52)), Ok(Value::Bool(false)));
    }

    /// Steps a slider, clamps it and commits the value.
    #[test]
    fn slider_steps_clamps_and_commits() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Timeout",
                Control::Slider {
                    minimum: 0,
                    maximum: 3,
                    step: 1,
                    unit: String::from("s"),
                    labels: vec![Choice::new("Off", 0)],
                },
            )
            .set_binding(Key(70));
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(70), Value::Number(0));
        let mut session = session_of(&set);
        assert!(session.enter(Key(1)));
        let view = session.view(&mut provider);
        assert_eq!(view.rows[0].value, "Off");
        assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
        assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
        assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
        let view = session.view(&mut provider);
        let editor = view.editor.expect("editor");
        let crate::session::EditorKind::Slider { ref text, .. } = editor.kind else {
            panic!("expected a slider editor");
        };
        assert_eq!(text, "2 s");
        assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
        assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
        assert_eq!(
            session.input(Input::Enter, &mut provider),
            Event::Response(Request::None)
        );
        assert_eq!(provider.store.read(Key(70)), Ok(Value::Number(3)));
    }

    /// Reports a periodic refresh only after its interval.
    #[test]
    fn periodic_refresh_waits_for_its_interval() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row("Clock", Control::Text(String::new()))
            .set_binding(Key(60))
            .refresh = crate::model::Refresh::Periodic(1_000);
        let set = builder.build();
        let mut provider = provider_of(&set);
        provider.set(Key(60), Value::Text(String::from("00:00")));
        let mut session = session_of(&set).with_readable_rows(true);
        assert!(session.enter(Key(1)));
        session.view(&mut provider);
        assert!(!session.poll(500));
        assert!(session.poll(1_500));
    }
}

#[cfg(test)]
mod live_tests {
    use super::{Event, Input, Session};
    use crate::ext::Decoration;
    use crate::model::{Builder, Choice, Control, FormSet, Key, MemoryProvider, Store, Value};
    use alloc::string::String;
    use alloc::vec;

    /// A slider row follows the editor before the value is written.
    #[test]
    fn a_slider_row_tracks_its_editor() {
        let mut builder = Builder::new(Key(1), "S");
        builder.page("Main");
        builder
            .row(
                "Fan",
                Control::Slider {
                    minimum: 0,
                    maximum: 3,
                    step: 1,
                    unit: String::from("s"),
                    labels: vec![Choice::new("Off", 0)],
                },
            )
            .set_binding(Key(80));
        let set: FormSet = builder.build();
        let mut provider = MemoryProvider::new(vec![(0u8, set.clone())]);
        provider.set(Key(80), Value::Number(0));
        let mut session = Session::new(vec![(0u8, set)]).with_back_row(false);
        assert!(session.enter(Key(1)));

        let view = session.view(&mut provider);
        assert_eq!(view.rows[0].value, "Off");
        assert_eq!(
            view.rows[0].decoration,
            Decoration::Slider {
                position: 0,
                minimum: 0,
                maximum: 3
            }
        );

        assert_eq!(session.input(Input::Enter, &mut provider), Event::Changed);
        assert_eq!(session.input(Input::Up, &mut provider), Event::Changed);
        let view = session.view(&mut provider);
        assert_eq!(view.rows[0].value, "1 s");
        assert_eq!(
            view.rows[0].decoration,
            Decoration::Slider {
                position: 1,
                minimum: 0,
                maximum: 3
            }
        );
        assert_eq!(
            provider.store.read(Key(80)),
            Ok(Value::Number(0)),
            "the row follows the editor before the value is written"
        );
    }
}
