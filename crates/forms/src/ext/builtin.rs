//! The handlers of the built-in control kinds.

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::model::{Choice, Control, Key, Value};
use crate::session::{Editor, EditorKind, Input};

use super::{Decoration, EditState, EditStep, Field, Handler, kind};

/// The label of the closing entry of an ordered list editor.
const DONE: &str = "Done";

/// Returns the kind of a control.
///
/// # Examples
///
/// ```
/// use forms::{Control, ext};
///
/// assert_eq!(ext::kind_of(&Control::Toggle), ext::kind::TOGGLE);
/// assert_eq!(
///     ext::kind_of(&Control::Custom { kind: forms::Key(5), data: Vec::new() }),
///     forms::Key(5)
/// );
/// ```
pub fn kind_of(control: &Control) -> Key {
    match control {
        Control::Heading => kind::HEADING,
        Control::Text(_) => kind::TEXT,
        Control::Link(_) => kind::LINK,
        Control::Action => kind::ACTION,
        Control::Toggle => kind::TOGGLE,
        Control::Choice(_) => kind::CHOICE,
        Control::Number { .. } => kind::NUMBER,
        Control::Slider { .. } => kind::SLIDER,
        Control::TextInput { secret: false, .. } => kind::TEXT_INPUT,
        Control::TextInput { secret: true, .. } => kind::PASSWORD,
        Control::OrderedList { .. } => kind::ORDERED_LIST,
        Control::Reset(_) => kind::RESET,
        Control::Progress { .. } => kind::PROGRESS,
        Control::Divider => kind::DIVIDER,
        Control::Unsupported(_) => kind::UNSUPPORTED,
        Control::Custom { kind, .. } => *kind,
    }
}

/// The kinds that [`super::Registry::builtin`] registers.
pub const BUILTIN_KINDS: [Key; 15] = [
    kind::HEADING,
    kind::TEXT,
    kind::LINK,
    kind::ACTION,
    kind::TOGGLE,
    kind::CHOICE,
    kind::NUMBER,
    kind::SLIDER,
    kind::TEXT_INPUT,
    kind::PASSWORD,
    kind::ORDERED_LIST,
    kind::RESET,
    kind::PROGRESS,
    kind::DIVIDER,
    kind::UNSUPPORTED,
];

/// Returns the handler of each built-in kind.
pub fn handlers() -> Vec<(Key, Box<dyn Handler>)> {
    alloc::vec![
        (kind::HEADING, Box::new(Heading) as Box<dyn Handler>),
        (kind::TEXT, Box::new(Text)),
        (kind::LINK, Box::new(LinkRow)),
        (kind::ACTION, Box::new(ActionRow)),
        (kind::TOGGLE, Box::new(Toggle)),
        (kind::CHOICE, Box::new(ChoiceOptions)),
        (kind::NUMBER, Box::new(Number)),
        (kind::SLIDER, Box::new(Slider)),
        (kind::TEXT_INPUT, Box::new(TextInput { secret: false })),
        (kind::PASSWORD, Box::new(TextInput { secret: true })),
        (kind::ORDERED_LIST, Box::new(OrderedList)),
        (kind::RESET, Box::new(Reset)),
        (kind::PROGRESS, Box::new(Progress)),
        (kind::DIVIDER, Box::new(Divider)),
        (kind::UNSUPPORTED, Box::new(Unsupported)),
    ]
}

/// Returns the text of a value, or an empty text.
fn text(value: Option<&Value>) -> &str {
    match value {
        Some(Value::Text(text)) => text,
        _ => "",
    }
}

/// Returns the number of a value, or 0.
fn number(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => *number,
        _ => 0,
    }
}

/// Returns the list of a value, or an empty list.
fn list(value: Option<&Value>) -> Vec<u64> {
    match value {
        Some(Value::List(values)) => values.clone(),
        _ => Vec::new(),
    }
}

/// Returns the label of one choice value.
fn choice_label(choices: &[Choice], value: u64) -> String {
    choices
        .iter()
        .find(|choice| choice.value == value)
        .map_or_else(|| format!("#{value}"), |choice| choice.label.clone())
}

/// A heading.
struct Heading;

impl Handler for Heading {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn decoration(&self, _field: &Field<'_>) -> Decoration {
        Decoration::Heading
    }
}

/// An information row.
struct Text;

impl Handler for Text {
    fn format(&self, field: &Field<'_>) -> String {
        let live = text(field.value);
        if live.is_empty() {
            match &field.row.control {
                Control::Text(text) => text.clone(),
                _ => String::new(),
            }
        } else {
            live.to_string()
        }
    }
}

/// A link row.
struct LinkRow;

impl Handler for LinkRow {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn edit(&self, _field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        if input == Input::Enter {
            EditStep::Invoke
        } else {
            EditStep::Ignored
        }
    }
}

/// An action row.
struct ActionRow;

impl Handler for ActionRow {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn edit(&self, _field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        if input == Input::Enter {
            EditStep::Invoke
        } else {
            EditStep::Ignored
        }
    }
}

/// A logical switch.
struct Toggle;

impl Handler for Toggle {
    fn format(&self, field: &Field<'_>) -> String {
        let on = matches!(field.value, Some(Value::Bool(true)));
        if on {
            String::from("On")
        } else {
            String::from("Off")
        }
    }

    fn decoration(&self, field: &Field<'_>) -> Decoration {
        Decoration::Toggle(matches!(field.value, Some(Value::Bool(true))))
    }

    fn edit(&self, field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        let on = matches!(field.value, Some(Value::Bool(true)));
        match input {
            Input::Enter | Input::Char(' ') | Input::Left | Input::Right => {
                EditStep::Accept(Value::Bool(!on))
            }
            _ => EditStep::Ignored,
        }
    }
}

/// One option of a choice list.
struct ChoiceOptions;

impl ChoiceOptions {
    /// Returns the options of a row.
    fn options<'a>(field: &'a Field<'_>) -> &'a [Choice] {
        match &field.row.control {
            Control::Choice(choices) => choices,
            _ => &[],
        }
    }

    /// Returns the index of the current value.
    fn index(field: &Field<'_>) -> usize {
        let options = Self::options(field);
        let value = number(field.value);
        options
            .iter()
            .position(|choice| choice.value == value)
            .unwrap_or(0)
    }
}

impl Handler for ChoiceOptions {
    fn format(&self, field: &Field<'_>) -> String {
        choice_label(Self::options(field), number(field.value))
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        EditState {
            selected: Self::index(field),
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        let items = Self::options(field)
            .iter()
            .map(|choice| choice.label.clone())
            .collect();
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::Choice {
                items,
                selected: state.selected,
            },
        })
    }

    fn edit(&self, field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        let options = Self::options(field);
        if options.is_empty() {
            return EditStep::Ignored;
        }
        let index = Self::index(field);
        let last = options.len() - 1;
        let next = match input {
            Input::Down | Input::Right => Some(if index == last { 0 } else { index + 1 }),
            Input::Up | Input::Left => Some(if index == 0 { last } else { index - 1 }),
            Input::Home => Some(0),
            _ => None,
        };
        if let Some(index) = next {
            let value = options[index].value;
            return EditStep::Accept(Value::Number(value));
        }
        EditStep::Ignored
    }
}

/// A bounded number.
struct Number;

impl Handler for Number {
    fn format(&self, field: &Field<'_>) -> String {
        format!("{}", number(field.value))
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        let text = format!("{}", number(field.value));
        EditState {
            cursor: text.chars().count(),
            text,
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::Number {
                text: state.text.clone(),
                cursor: state.cursor,
            },
        })
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (minimum, maximum, step) = match &field.row.control {
            Control::Number {
                minimum,
                maximum,
                step,
            } => (*minimum, *maximum, *step),
            _ => (0, u64::MAX, 0),
        };
        match input {
            Input::Char(character) if character.is_ascii_digit() => {
                if state.text.chars().count() >= 20 {
                    return EditStep::Ignored;
                }
                state.text.insert(state.cursor, character);
                state.cursor += 1;
                EditStep::Changed
            }
            Input::Erase => {
                if state.cursor == 0 {
                    return EditStep::Ignored;
                }
                state.cursor -= 1;
                state.text.remove(state.cursor);
                EditStep::Changed
            }
            // Up and Down step the value, so a device without a keyboard can
            // set a number; Left and Right move the cursor.
            Input::Up | Input::Down => {
                let current = state.text.parse::<u64>().unwrap_or(minimum);
                let step = step.max(1);
                let next = if input == Input::Up {
                    current.saturating_add(step).min(maximum)
                } else {
                    current.saturating_sub(step).max(minimum)
                };
                state.text = next.to_string();
                state.cursor = state.text.chars().count();
                EditStep::Changed
            }
            Input::Left => {
                state.cursor = state.cursor.saturating_sub(1);
                EditStep::Changed
            }
            Input::Right => {
                state.cursor = state
                    .cursor
                    .saturating_add(1)
                    .min(state.text.chars().count());
                EditStep::Changed
            }
            Input::Enter => {
                let Ok(value) = state.text.parse::<u64>() else {
                    return EditStep::Refused(String::from("Enter a number"));
                };
                if value < minimum || value > maximum {
                    return EditStep::Refused(format!("Enter a value from {minimum} to {maximum}"));
                }
                if step != 0 && (value - minimum) % step != 0 {
                    return EditStep::Refused(format!(
                        "Enter a value that is a multiple of {step} from {minimum}"
                    ));
                }
                EditStep::Accept(Value::Number(value))
            }
            _ => EditStep::Ignored,
        }
    }
}

/// A bounded number that two keys adjust.
struct Slider;

impl Slider {
    /// Returns the limits, the step, the unit and the labels of a row.
    fn parts(field: &Field<'_>) -> (u64, u64, u64, String, Vec<Choice>) {
        match &field.row.control {
            Control::Slider {
                minimum,
                maximum,
                step,
                unit,
                labels,
            } => (
                *minimum,
                *maximum,
                (*step).max(1),
                unit.clone(),
                labels.clone(),
            ),
            _ => (0, u64::MAX, 1, String::new(), Vec::new()),
        }
    }

    /// Returns the displayed text of one value.
    fn text(value: u64, unit: &str, labels: &[Choice]) -> String {
        if let Some(choice) = labels.iter().find(|choice| choice.value == value) {
            return choice.label.clone();
        }
        if unit.is_empty() {
            format!("{value}")
        } else {
            format!("{value} {unit}")
        }
    }
}

impl Handler for Slider {
    fn format(&self, field: &Field<'_>) -> String {
        let (_, _, _, unit, labels) = Self::parts(field);
        Self::text(number(field.value), &unit, &labels)
    }

    fn decoration(&self, field: &Field<'_>) -> Decoration {
        let (minimum, maximum, _, _, _) = Self::parts(field);
        let position = number(field.value).clamp(minimum, maximum);
        Decoration::Slider {
            position,
            minimum,
            maximum,
        }
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        EditState {
            value: number(field.value),
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        let (minimum, maximum, _, unit, labels) = Self::parts(field);
        let position = state.value.clamp(minimum, maximum);
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::Slider {
                text: Self::text(position, &unit, &labels),
                position,
                minimum,
                maximum,
            },
        })
    }

    fn format_editing(&self, field: &Field<'_>, state: &EditState) -> Option<(String, Decoration)> {
        let (minimum, maximum, _, unit, labels) = Self::parts(field);
        let position = state.value.clamp(minimum, maximum);
        Some((
            Self::text(position, &unit, &labels),
            Decoration::Slider {
                position,
                minimum,
                maximum,
            },
        ))
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (minimum, maximum, step, _, _) = Self::parts(field);
        match input {
            Input::Up | Input::Right => {
                state.value = state.value.saturating_add(step).min(maximum);
                EditStep::Changed
            }
            Input::Down | Input::Left => {
                state.value = state.value.saturating_sub(step).max(minimum);
                EditStep::Changed
            }
            Input::Home => {
                state.value = minimum;
                EditStep::Changed
            }
            Input::End => {
                state.value = maximum;
                EditStep::Changed
            }
            Input::Enter => EditStep::Accept(Value::Number(state.value.clamp(minimum, maximum))),
            _ => EditStep::Ignored,
        }
    }
}
/// A text or password value with full keyboard editing.
struct TextInput {
    /// Conceal the text.
    secret: bool,
}

impl TextInput {
    /// Returns the length limits of a row.
    fn limits(field: &Field<'_>) -> (usize, usize) {
        match &field.row.control {
            Control::TextInput {
                minimum, maximum, ..
            } => (*minimum, *maximum),
            _ => (0, usize::MAX),
        }
    }
}

impl Handler for TextInput {
    fn format(&self, field: &Field<'_>) -> String {
        let text = text(field.value);
        if self.secret {
            "*".repeat(text.chars().count())
        } else {
            text.to_string()
        }
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        let text = text(field.value).to_string();
        EditState {
            cursor: text.chars().count(),
            text,
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::Text {
                text: state.text.clone(),
                cursor: state.cursor,
                secret: self.secret,
            },
        })
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (minimum, maximum) = Self::limits(field);
        let length = state.text.chars().count();
        match input {
            Input::Char(character) => {
                if length >= maximum {
                    return EditStep::Refused(format!(
                        "The text accepts at most {maximum} characters"
                    ));
                }
                state.text.insert(state.cursor, character);
                state.cursor += 1;
                EditStep::Changed
            }
            Input::Erase => {
                if state.cursor == 0 {
                    return EditStep::Ignored;
                }
                state.cursor -= 1;
                state.text.remove(state.cursor);
                EditStep::Changed
            }
            Input::Left => {
                state.cursor = state.cursor.saturating_sub(1);
                EditStep::Changed
            }
            Input::Right => {
                state.cursor = state.cursor.saturating_add(1).min(length);
                EditStep::Changed
            }
            Input::Home => {
                state.cursor = 0;
                EditStep::Changed
            }
            Input::End => {
                state.cursor = length;
                EditStep::Changed
            }
            Input::Enter => {
                if length < minimum {
                    return EditStep::Refused(format!(
                        "The text needs at least {minimum} characters"
                    ));
                }
                EditStep::Accept(Value::Text(state.text.clone()))
            }
            _ => EditStep::Ignored,
        }
    }
}

/// An ordered list that a person reorders and selects from.
struct OrderedList;

impl OrderedList {
    /// Returns the options and the limit of a row.
    fn options<'a>(field: &'a Field<'_>) -> (&'a [Choice], usize) {
        match &field.row.control {
            Control::OrderedList { choices, maximum } => {
                let maximum = if *maximum == 0 {
                    choices.len()
                } else {
                    *maximum
                };
                (choices, maximum)
            }
            _ => (&[], 0),
        }
    }

    /// Returns the chosen values and the available option values.
    fn split(choices: &[Choice], chosen: &[u64]) -> (Vec<u64>, Vec<u64>) {
        let available: Vec<u64> = choices
            .iter()
            .map(|choice| choice.value)
            .filter(|value| !chosen.contains(value))
            .collect();
        (chosen.to_vec(), available)
    }

    /// Returns the number of entries, including the closing entry.
    fn entries(choices: &[Choice], chosen: &[u64]) -> usize {
        let (chosen, available) = Self::split(choices, chosen);
        chosen.len() + available.len() + 1
    }
}

impl Handler for OrderedList {
    fn format(&self, field: &Field<'_>) -> String {
        let (choices, _) = Self::options(field);
        let chosen = list(field.value);
        chosen
            .iter()
            .map(|value| choice_label(choices, *value))
            .collect::<Vec<String>>()
            .join(", ")
    }

    fn opens_editor(&self) -> bool {
        true
    }

    fn start(&self, field: &Field<'_>) -> EditState {
        EditState {
            values: list(field.value),
            ..EditState::default()
        }
    }

    fn editor(&self, field: &Field<'_>, state: &EditState) -> Option<Editor> {
        let (choices, _) = Self::options(field);
        let (chosen, available) = Self::split(choices, &state.values);
        let mut items: Vec<String> = chosen
            .iter()
            .map(|value| choice_label(choices, *value))
            .collect();
        items.extend(available.iter().map(|value| choice_label(choices, *value)));
        items.push(String::from(DONE));
        Some(Editor {
            row: field.row.key,
            label: field.row.label.clone(),
            kind: EditorKind::List {
                items,
                selected: state.selected.min(chosen.len() + available.len()),
                chosen: chosen.len(),
                held: state.held && state.selected < chosen.len(),
            },
        })
    }

    fn edit(&self, field: &Field<'_>, state: &mut EditState, input: Input) -> EditStep {
        let (choices, maximum) = Self::options(field);
        let total = Self::entries(choices, &state.values);
        let closing = total - 1;
        if state.selected > closing {
            state.selected = closing;
        }
        let (chosen, available) = Self::split(choices, &state.values);
        // A device with only volume keys and power works the list with Up,
        // Down and Enter: Enter picks a chosen entry up, Up and Down move it,
        // Enter drops it, and Down past the last chosen entry removes it.
        // Left and Right stay as keyboard shortcuts for one move.
        if state.held && state.selected < chosen.len() {
            return match input {
                Input::Up | Input::Left => {
                    if state.selected > 0 {
                        state.values.swap(state.selected, state.selected - 1);
                        state.selected -= 1;
                    }
                    EditStep::Changed
                }
                Input::Down | Input::Right => {
                    if state.selected + 1 < chosen.len() {
                        state.values.swap(state.selected, state.selected + 1);
                        state.selected += 1;
                    } else if input == Input::Down {
                        state.values.remove(state.selected);
                        state.held = false;
                        state.selected = state.values.len();
                    }
                    EditStep::Changed
                }
                Input::Enter => {
                    state.held = false;
                    EditStep::Changed
                }
                _ => EditStep::Ignored,
            };
        }
        state.held = false;
        match input {
            Input::Up => {
                state.selected = if state.selected == 0 {
                    closing
                } else {
                    state.selected - 1
                };
                EditStep::Changed
            }
            Input::Down => {
                state.selected = if state.selected == closing {
                    0
                } else {
                    state.selected + 1
                };
                EditStep::Changed
            }
            Input::Left => {
                if state.selected == 0 || state.selected >= chosen.len() {
                    return EditStep::Ignored;
                }
                state.values.swap(state.selected, state.selected - 1);
                state.selected -= 1;
                EditStep::Changed
            }
            Input::Right => {
                if state.selected + 1 >= chosen.len() {
                    return EditStep::Ignored;
                }
                state.values.swap(state.selected, state.selected + 1);
                state.selected += 1;
                EditStep::Changed
            }
            Input::Enter if state.selected < chosen.len() => {
                state.held = true;
                EditStep::Changed
            }
            Input::Enter if state.selected < closing => {
                if state.values.len() >= maximum {
                    return EditStep::Refused(format!("Select at most {maximum} values"));
                }
                let index = state.selected - chosen.len();
                if let Some(value) = available.get(index) {
                    state.values.push(*value);
                    state.selected = state.values.len() - 1;
                }
                EditStep::Changed
            }
            Input::Enter => EditStep::Accept(Value::List(state.values.clone())),
            _ => EditStep::Ignored,
        }
    }
}

/// A reset command.
struct Reset;

impl Handler for Reset {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn edit(&self, _field: &Field<'_>, _state: &mut EditState, input: Input) -> EditStep {
        if input == Input::Enter {
            EditStep::Invoke
        } else {
            EditStep::Ignored
        }
    }
}

/// A progress or busy row.
struct Progress;

impl Handler for Progress {
    fn format(&self, field: &Field<'_>) -> String {
        match &field.row.control {
            Control::Progress { current, total } => match total {
                Some(total) if *total > 0 => {
                    format!("{}/{}", current.min(total), total)
                }
                _ => String::from("Working"),
            },
            _ => String::new(),
        }
    }

    fn decoration(&self, field: &Field<'_>) -> Decoration {
        match &field.row.control {
            Control::Progress { current, total } => Decoration::Bar {
                current: *current,
                total: *total,
            },
            _ => Decoration::Plain,
        }
    }
}

/// A divider.
struct Divider;

impl Handler for Divider {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }

    fn decoration(&self, _field: &Field<'_>) -> Decoration {
        Decoration::Rule
    }
}

/// An imported control that the crate cannot show.
struct Unsupported;

impl Handler for Unsupported {
    fn format(&self, _field: &Field<'_>) -> String {
        String::new()
    }
}
