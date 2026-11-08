//! The keys of a device: what the session offers and what the hint line names.

/// The keys of one device.
///
/// One value states the navigation rule of a device, so a session and the
/// page that draws it agree. A device without a Back key leaves an editor
/// unchanged when Enter accepts the value it opened with, and every page keeps
/// a Back row. A device without a keyboard opens no editor that needs typed
/// text. The rows that need typing stay visible, grayed out, and their help
/// states why.
///
/// # Examples
///
/// ```
/// use forms::Keypad;
///
/// let phone = Keypad::volume_and_power();
/// assert_eq!(phone.back, None);
/// assert!(!phone.typing);
/// assert_eq!(Keypad::default().back, Some("Esc"));
/// assert!(Keypad::default().typing);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keypad {
    /// The name of the keys that move the selection.
    pub move_keys: &'static str,
    /// The name of the key that selects or accepts.
    pub select: &'static str,
    /// The name of the key that cancels or goes back, or `None` when the device
    /// has no such key.
    pub back: Option<&'static str>,
    /// The device can type text.
    pub typing: bool,
}

impl Keypad {
    /// Returns the keys of a keyboard.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Keypad::keyboard().select, "Enter");
    /// ```
    #[must_use]
    pub const fn keyboard() -> Self {
        Self {
            move_keys: "Up Down",
            select: "Enter",
            back: Some("Esc"),
            typing: true,
        }
    }

    /// Returns the keys of a device with only volume keys and a power key.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(forms::Keypad::volume_and_power().select, "Power");
    /// ```
    #[must_use]
    pub const fn volume_and_power() -> Self {
        Self {
            move_keys: "Vol",
            select: "Power",
            back: None,
            typing: false,
        }
    }
}

impl Default for Keypad {
    fn default() -> Self {
        Self::keyboard()
    }
}
