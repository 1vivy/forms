//! Moving the selection, opening rows and leaving pages.

use super::*;

impl<H: Copy + Eq> Session<H> {
    /// Moves the selection within the visible rows.
    pub(super) fn move_selection<P: Provider<Handle = H>>(
        &mut self,
        input: Input,
        provider: &mut P,
    ) -> Event {
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
    pub(super) fn activate<P: Provider<Handle = H>>(&mut self, provider: &mut P) -> Event {
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
        // The Back row is the session's own row, not a page row.
        if self.back_row && selected == back_row().key {
            return self.back(provider);
        }
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
        if self.untypable(&row) {
            let error = Error::new(super::view::NEEDS_KEYBOARD);
            self.message = Some(error.reason.clone());
            return Event::Failed(error);
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

    /// Leaves a page after a request that asks for it.
    pub(super) fn leave(&mut self, scope: Scope) {
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
    pub(super) fn back<P: Provider<Handle = H>>(&mut self, provider: &mut P) -> Event {
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
}
