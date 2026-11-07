//! Navigation, editing, validation and modal state.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec,
    vec::Vec,
};

use crate::controls::{Decoration, EditState, EditStep, Field, Registry};
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

mod edit;
mod navigate;
#[cfg(test)]
mod tests;
mod view;

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

    /// Returns the handle, set and page of the active page.
    fn current(&self) -> Option<(H, FormSet, Key)> {
        let page = self.pages.last()?;
        let (_, set) = self
            .sets
            .iter()
            .find(|(handle, set)| *handle == page.handle && set.key == page.set)?;
        Some((page.handle, set.clone(), page.page))
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
