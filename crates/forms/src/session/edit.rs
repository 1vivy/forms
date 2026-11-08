//! Editors, commands, validation, resets and the requests that providers return.

use super::*;

impl<H: Copy + Eq> Session<H> {
    /// Applies one editor input.
    pub(super) fn edit<P: Provider<Handle = H>>(
        &mut self,
        input: Input,
        provider: &mut P,
    ) -> Event {
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
                if self.keypad.back.is_none() && open.value.as_ref() == Some(&value) =>
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
    pub(super) fn apply_step<P: Provider<Handle = H>>(
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
    pub(super) fn invoke<P: Provider<Handle = H>>(
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
    pub(super) fn commit<P: Provider<Handle = H>>(
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
    pub(super) fn reset<P: Provider<Handle = H>>(
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
    pub(super) fn run_request<P: Provider<Handle = H>>(
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

    /// Checks the submit rules of the active page.
    pub(super) fn check_submit<P: Provider<Handle = H>>(
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
    pub(super) fn check<P: Provider<Handle = H>>(
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
    pub(super) fn evaluate<P: Provider<Handle = H>>(
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
}
