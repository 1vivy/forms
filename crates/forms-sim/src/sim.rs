//! One page that a front end drives one key stroke at a time.
//!
//! [`Sim`] joins a [`Session`], a provider and a [`Screen`], so a consumer that
//! has no firmware display writes a few lines. [`Defaults`] answers every
//! question from the statements of a formset, for a page whose values are not
//! stored anywhere.
//!
//! A page that opens confirmation popups keeps its own policy: it drives the
//! [`Session`] and the [`Provider`] directly and draws the view that
//! `Session::view` returns, as the ODP setup front end does.

use forms::{Event, FormSet, Guid, Input, Provider, Question, Session, Statement, Value, View};
use std::string::ToString;
use std::vec::Vec;

use crate::{Config, Screen};

/// A page with a screen, a session and a provider.
pub struct Sim<H: Copy + Eq, P: Provider<Handle = H>> {
    /// The navigation state.
    session: Session<H>,
    /// The provider behind the page.
    provider: P,
    /// The panel that shows the page.
    screen: Screen,
    /// The line that states the last event.
    notice: String,
}

impl<H: Copy + Eq, P: Provider<Handle = H>> Sim<H, P> {
    /// Opens the entry form of `entry` over `provider`.
    ///
    /// The session shows the first form of that formset. A formset that the
    /// list does not hold leaves the session on its list page.
    ///
    /// # Examples
    ///
    /// ```
    /// use forms_sim::{Config, Defaults, Sim};
    ///
    /// let set = forms_sim::sample();
    /// let mut sim = Sim::new(&set, Defaults::new(&set), forms_sim::SHARED, Config::default());
    /// sim.press(forms::Input::Down);
    /// assert_eq!(sim.notice(), "changed");
    /// ```
    pub fn new(set: &FormSet, provider: P, handle: H, config: Config) -> Self {
        let mut session = Session::new(vec![(handle, set.clone())]);
        let _ = session.enter(&set.guid);
        Self {
            session,
            provider,
            screen: Screen::new(config),
            notice: String::new(),
        }
    }

    /// Returns the page that the screen shows.
    pub fn view(&mut self) -> View {
        self.session.view(&mut self.provider)
    }

    /// Applies one key stroke, records what it changed and draws the page.
    ///
    /// The method returns the event, so a caller can act on an
    /// [`Event::Response`](forms::Event::Response) or a
    /// [`Event::Popup`](forms::Event::Popup) itself.
    ///
    /// # Errors
    ///
    /// Returns the drawing failure of the screen. The event still applies, and
    /// the notice states the failure.
    pub fn press(&mut self, input: Input) -> Event {
        let event = self.session.input(input, &mut self.provider);
        self.notice = describe(&event);
        // A failed draw states itself in the notice.
        let _ = self.draw();
        event
    }

    /// Draws the page from a cleared screen.
    ///
    /// # Errors
    ///
    /// Returns the drawing failure of the screen.
    pub fn draw(&mut self) -> Result<(), mousefood::error::Error> {
        let view = self.session.view(&mut self.provider);
        let drawn = self.screen.draw(true, &view);
        if drawn.is_err() {
            self.notice = String::from("the view did not draw");
        }
        drawn
    }

    /// Returns the line that states the last event.
    #[must_use]
    pub fn notice(&self) -> &str {
        &self.notice
    }

    /// Returns the panel that shows the page.
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Returns the navigation state of the page.
    pub fn session(&mut self) -> &mut Session<H> {
        &mut self.session
    }
}

/// A provider that answers every question from the statements of a formset.
///
/// A checkbox states its default and a one-of states its default choice. Every
/// other question answers `0`, and a write stores the value that the page
/// commits. This provider suits a page that shows a formset and no stored
/// settings.
///
/// # Examples
///
/// ```
/// use forms::Provider;
/// use forms_sim::Defaults;
///
/// let set = forms_sim::sample();
/// let mut provider = Defaults::new(&set);
/// assert_eq!(provider.formsets().expect("the list reads").len(), 1);
/// ```
pub struct Defaults {
    /// The formset that the provider answers for.
    set: FormSet,
    /// The stored value of each question.
    values: Vec<(u16, Value)>,
}

impl Defaults {
    /// Returns a provider over the defaults of `set`.
    #[must_use]
    pub fn new(set: &FormSet) -> Self {
        let mut provider = Self {
            set: set.clone(),
            values: Vec::new(),
        };
        for form in &set.forms {
            for item in &form.items {
                match &item.statement {
                    Statement::Checkbox { question, default } => {
                        provider.values.push((question.id.0, Value::Bool(*default)));
                    }
                    Statement::OneOf {
                        question, choices, ..
                    } => {
                        let chosen = choices
                            .iter()
                            .find(|choice| choice.default)
                            .or(choices.first());
                        if let Some(choice) = chosen {
                            provider
                                .values
                                .push((question.id.0, Value::Number(choice.value)));
                        }
                    }
                    _ => {}
                }
            }
        }
        provider
    }

    /// Returns the stored value of one question.
    fn get(&self, question: u16) -> Value {
        self.values
            .iter()
            .find(|(id, _)| *id == question)
            .map_or(Value::Number(0), |(_, value)| value.clone())
    }
}

impl Provider for Defaults {
    type Handle = u8;

    fn formsets(&mut self) -> Result<Vec<(u8, FormSet)>, forms::Error> {
        Ok(vec![(SHARED, self.set.clone())])
    }

    fn read(
        &mut self,
        _handle: u8,
        _set: &FormSet,
        question: &Question,
    ) -> Result<Value, forms::Error> {
        Ok(self.get(question.id.0))
    }

    fn write(
        &mut self,
        _handle: u8,
        _set: &FormSet,
        question: &Question,
        value: &Value,
    ) -> Result<forms::ActionRequest, forms::Error> {
        match self.values.iter_mut().find(|(id, _)| *id == question.id.0) {
            Some((_, current)) => *current = value.clone(),
            None => self.values.push((question.id.0, value.clone())),
        }
        Ok(forms::ActionRequest::None)
    }

    fn action(
        &mut self,
        _handle: u8,
        _set: &FormSet,
        _question: &Question,
    ) -> Result<forms::ActionRequest, forms::Error> {
        Ok(forms::ActionRequest::None)
    }
}

/// The handle that [`Defaults`] uses for its one formset.
pub const SHARED: u8 = 1;

/// Returns a short line that states what one event changed.
///
/// # Examples
///
/// ```
/// use forms::Event;
/// use forms_sim::describe;
///
/// assert_eq!(describe(&Event::None), "");
/// assert_eq!(describe(&Event::Exit), "exit");
/// ```
#[must_use]
pub fn describe(event: &Event) -> String {
    match event {
        Event::None => String::new(),
        Event::Changed => String::from("changed"),
        Event::Exit => String::from("exit"),
        Event::Response(request) => format!("request: {request:?}"),
        Event::Popup(selection) => format!("popup: {selection:?}"),
        Event::Failed(error) => format!("failed: {}", error.reason),
    }
}

/// Returns the input of one browser key stroke.
///
/// `code` is `0` for Up, `1` for Down, `2` for Left, `3` for Right, `4` for
/// Home, `5` for Enter, `6` for Back, `7` for Erase and `8` for the character
/// in `character`. Every other code returns `None`.
///
/// # Examples
///
/// ```
/// use forms::Input;
/// use forms_sim::input;
///
/// assert_eq!(input(0, 0), Some(Input::Up));
/// assert_eq!(input(8, u32::from(b'k')), Some(Input::Char('k')));
/// assert_eq!(input(99, 0), None);
/// ```
#[must_use]
pub fn input(code: u32, character: u32) -> Option<Input> {
    Some(match code {
        0 => Input::Up,
        1 => Input::Down,
        2 => Input::Left,
        3 => Input::Right,
        4 => Input::Home,
        5 => Input::Enter,
        6 => Input::Back,
        7 => Input::Erase,
        8 => Input::Char(char::from_u32(character)?),
        _ => return None,
    })
}

/// Returns the title of a formset, for a page that states it.
///
/// # Examples
///
/// ```
/// let set = forms_sim::sample();
/// assert_eq!(forms_sim::title(&set), "Sample");
/// ```
#[must_use]
pub fn title(set: &FormSet) -> String {
    set.title.to_string()
}

/// Returns the identifier of a formset.
///
/// # Examples
///
/// ```
/// let set = forms_sim::sample();
/// assert_eq!(forms_sim::guid(&set), [0x33; 16]);
/// ```
#[must_use]
pub fn guid(set: &FormSet) -> Guid {
    set.guid
}

#[cfg(test)]
mod tests {
    use super::{Defaults, SHARED, Sim, describe, input};
    use crate::Config;
    use forms::{Event, Input, Provider, Value};

    #[test]
    fn a_sim_presses_and_reports() {
        let set = crate::sample();
        let mut sim = Sim::new(&set, Defaults::new(&set), SHARED, Config::default());
        assert_eq!(sim.press(Input::Down), Event::Changed);
        assert_eq!(sim.notice(), "changed");
        assert!(sim.draw().is_ok());
        let (width, height) = Config::default().panel;
        assert_eq!(
            sim.screen().pixels().len(),
            width as usize * height as usize * 3
        );
        assert_eq!(sim.session().view(&mut Defaults::new(&set)).title, "Sample");
    }

    #[test]
    fn the_defaults_provider_answers_from_the_formset() {
        let set = crate::sample();
        let mut provider = Defaults::new(&set);
        assert_eq!(provider.formsets().expect("the list reads").len(), 1);
        let question = forms::Question {
            id: forms::QuestionId(1),
            prompt: String::from("Row"),
            help: String::new(),
            storage: forms::Storage::None,
            flags: forms::QuestionFlags(forms::QuestionFlags::CALLBACK.0),
        };
        assert_eq!(
            provider
                .read(SHARED, &set, &question)
                .expect("the value reads"),
            Value::Bool(false)
        );
        let written = provider
            .write(SHARED, &set, &question, &Value::Bool(true))
            .expect("the value writes");
        assert_eq!(written, forms::ActionRequest::None);
        assert_eq!(
            provider
                .read(SHARED, &set, &question)
                .expect("the value reads"),
            Value::Bool(true)
        );
    }

    #[test]
    fn an_event_and_a_key_stroke_have_one_description() {
        assert_eq!(describe(&Event::None), "");
        assert_eq!(input(5, 0), Some(Input::Enter));
        assert_eq!(input(42, 0), None);
    }
}
