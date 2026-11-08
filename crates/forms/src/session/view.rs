//! Building the view a renderer draws, and reading the values and conditions it shows.

use super::*;

impl View {
    /// Returns the page as plain text lines: what a person sees, without a
    /// display.
    ///
    /// The first line is the path and the title. Each row follows as
    /// `label: value`, with `>` before the selected row and `(off)` after a
    /// row that accepts no change. An open editor, an open modal, the help and
    /// the message follow the rows. A host reads these lines to review or
    /// drive a page that only a panel shows.
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
    /// provider.set(Key(10), forms::Value::Bool(true));
    /// let mut session = Session::new(provider.sets.clone()).with_back_row(false);
    /// session.enter(Key(1));
    /// let lines = session.view(&mut provider).lines();
    /// assert_eq!(lines[0], "Main");
    /// assert_eq!(lines[1], "> Secure boot: On");
    /// ```
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut head = self.path.join(" > ");
        if !head.is_empty() {
            head.push_str(" > ");
        }
        head.push_str(&self.title);
        lines.push(head);
        for (index, row) in self.rows.iter().enumerate() {
            let cursor = if self.selected == Some(index) {
                "> "
            } else {
                "  "
            };
            let mut line = alloc::format!("{cursor}{}", row.label);
            if !row.value.is_empty() {
                line.push_str(": ");
                line.push_str(&row.value);
            }
            if !row.enabled {
                line.push_str(" (off)");
            }
            lines.push(line);
        }
        if let Some(editor) = &self.editor {
            lines.push(alloc::format!("editor {}:", editor.label));
            match &editor.kind {
                EditorKind::Choice {
                    items,
                    selected,
                    current,
                } => {
                    for (index, item) in items.iter().enumerate() {
                        let cursor = if index == *selected { '>' } else { ' ' };
                        let mark = if index == *current { '*' } else { ' ' };
                        lines.push(alloc::format!("  {cursor}{mark} {item}"));
                    }
                }
                EditorKind::Number { text, .. }
                | EditorKind::Text {
                    text,
                    secret: false,
                    ..
                } => {
                    lines.push(alloc::format!("  {text}"));
                }
                EditorKind::Text { text, .. } => {
                    lines.push(alloc::format!("  {}", "*".repeat(text.chars().count())));
                }
                EditorKind::Slider { text, .. } => lines.push(alloc::format!("  {text}")),
                EditorKind::List {
                    items,
                    selected,
                    chosen,
                    ..
                } => {
                    for (index, item) in items.iter().enumerate() {
                        let cursor = if index == *selected { '>' } else { ' ' };
                        let mark = if index < *chosen { '*' } else { ' ' };
                        lines.push(alloc::format!("  {cursor}{mark} {item}"));
                    }
                }
            }
        }
        if let Some(modal) = &self.modal {
            lines.push(alloc::format!("modal {}: {}", modal.title, modal.body));
            for (index, button) in modal.buttons.iter().enumerate() {
                let cursor = if index == modal.selected { '>' } else { ' ' };
                lines.push(alloc::format!("  {cursor} [{}]", button.label));
            }
        }
        if !self.help.is_empty() {
            lines.push(alloc::format!("help: {}", self.help));
        }
        if let Some(message) = &self.message {
            lines.push(alloc::format!("message: {message}"));
        }
        lines
    }
}

impl<H: Copy + Eq> Session<H> {
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
                let untypable = self.untypable(row);
                let selectable = row.control.is_interactive()
                    && !disabled
                    && (self.readable_rows || !matches!(row.control, Control::Text(_)));
                if selectable && !any_selected && selected_key == Key::default() {
                    selected_key = row.key;
                    any_selected = true;
                }
                let help = if untypable {
                    untypable_help(&row.help)
                } else {
                    row.help.clone()
                };
                rows.push(RowView {
                    key: row.key,
                    label: row.label.clone(),
                    value: text,
                    help,
                    decoration,
                    selectable,
                    link: matches!(row.control, Control::Link(_)),
                    enabled: !disabled && !locked && !untypable,
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

    /// Evaluates an optional condition. An absent condition is false.
    pub(super) fn condition<P: Provider<Handle = H>>(
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

    /// Returns whether a row needs typed text on a device that cannot type.
    pub(super) fn untypable(&self, row: &Row) -> bool {
        !self.keypad.typing
            && self
                .registry
                .handler(&row.control)
                .is_some_and(|handler| handler.needs_typing())
    }

    /// Returns whether a row is read-only.
    pub(super) fn read_only<P: Provider<Handle = H>>(
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
    pub(super) fn read_value<P: Provider<Handle = H>>(
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

    /// Builds the editor view of the open editor.
    pub(super) fn editor_view(&self, set: &FormSet) -> Option<Editor> {
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
    pub(super) fn root_view(&self) -> View {
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

/// The reason a row that needs typing is grayed out.
pub(super) const NEEDS_KEYBOARD: &str = "Needs a keyboard.";

/// The help of a row that needs typing on a device that cannot type.
///
/// The row's own help follows the reason, so a consumer can state another
/// way to set the value.
fn untypable_help(help: &str) -> String {
    if help.is_empty() {
        String::from(NEEDS_KEYBOARD)
    } else {
        alloc::format!("{NEEDS_KEYBOARD} {help}")
    }
}
