//! Building the view a renderer draws, and reading the values and conditions it shows.

use super::*;

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
