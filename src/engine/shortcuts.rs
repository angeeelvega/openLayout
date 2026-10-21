use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mod {
    bits: u8,
}

impl Mod {
    pub const CONTROL: u8 = 1 << 0;
    pub const OPTION: u8 = 1 << 1;
    pub const SHIFT: u8 = 1 << 2;
    pub const COMMAND: u8 = 1 << 3;

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn new(bits: u8) -> Self {
        Self { bits }
    }

    pub const fn contains(self, flag: u8) -> bool {
        self.bits & flag == flag
    }

    pub const fn union(self, other: Self) -> Self {
        Self { bits: self.bits | other.bits }
    }

    pub const fn bits(self) -> u8 {
        self.bits
    }

    pub fn symbols(self) -> String {
        let mut text = String::new();
        if self.contains(Self::CONTROL) {
            text.push('⌃');
        }
        if self.contains(Self::OPTION) {
            text.push('⌥');
        }
        if self.contains(Self::SHIFT) {
            text.push('⇧');
        }
        if self.contains(Self::COMMAND) {
            text.push('⌘');
        }
        text
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    FocusFirst,
    FocusLast,
    FocusNextDisplay,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveToNextDisplay,
    Jump1,
    Jump2,
    Jump3,
    Jump4,
    Jump5,
    Jump6,
    Jump7,
    Jump8,
    Jump9,
    CycleWidth,
    FullWidth,
    CenterColumn,
    Wider,
    Narrower,
    Taller,
    Shorter,
    EqualizeHeights,
    PinLeft,
    PinRight,
    StackIntoLeft,
    StackIntoRight,
    PopOutLeft,
    PopOutRight,
    ToggleTabs,
    ToggleFloat,
    TogglePause,
    ToggleOverview,
    CycleProfile,
}

impl Action {
    pub fn jump_index(self) -> Option<usize> {
        Some(match self {
            Self::Jump1 => 0,
            Self::Jump2 => 1,
            Self::Jump3 => 2,
            Self::Jump4 => 3,
            Self::Jump5 => 4,
            Self::Jump6 => 5,
            Self::Jump7 => 6,
            Self::Jump8 => 7,
            Self::Jump9 => 8,
            _ => return None,
        })
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::FocusLeft => "Focus left",
            Self::FocusRight => "Focus right",
            Self::FocusUp => "Focus up",
            Self::FocusDown => "Focus down",
            Self::FocusFirst => "Focus first",
            Self::FocusLast => "Focus last",
            Self::FocusNextDisplay => "Focus next display",
            Self::MoveLeft => "Move left",
            Self::MoveRight => "Move right",
            Self::MoveUp => "Move up",
            Self::MoveDown => "Move down",
            Self::MoveToNextDisplay => "Send to next display",
            Self::Jump1 => "Jump to column 1",
            Self::Jump2 => "Jump to column 2",
            Self::Jump3 => "Jump to column 3",
            Self::Jump4 => "Jump to column 4",
            Self::Jump5 => "Jump to column 5",
            Self::Jump6 => "Jump to column 6",
            Self::Jump7 => "Jump to column 7",
            Self::Jump8 => "Jump to column 8",
            Self::Jump9 => "Jump to column 9",
            Self::CycleWidth => "Cycle width",
            Self::FullWidth => "Full width",
            Self::CenterColumn => "Center column",
            Self::Wider => "Wider",
            Self::Narrower => "Narrower",
            Self::Taller => "Taller",
            Self::Shorter => "Shorter",
            Self::EqualizeHeights => "Equalize heights",
            Self::PinLeft => "Pin left",
            Self::PinRight => "Pin right",
            Self::StackIntoLeft => "Stack into left column",
            Self::StackIntoRight => "Stack into right column",
            Self::PopOutLeft => "Pop out left",
            Self::PopOutRight => "Pop out right",
            Self::ToggleTabs => "Toggle tabs",
            Self::ToggleFloat => "Float or tile",
            Self::TogglePause => "Pause or resume",
            Self::ToggleOverview => "Show every display",
            Self::CycleProfile => "Next profile",
        }
    }

    pub fn all() -> &'static [Action] {
        &ALL
    }
}

const ALL: [Action; 40] = [
    Action::FocusLeft,
    Action::FocusRight,
    Action::FocusUp,
    Action::FocusDown,
    Action::FocusFirst,
    Action::FocusLast,
    Action::FocusNextDisplay,
    Action::MoveLeft,
    Action::MoveRight,
    Action::MoveUp,
    Action::MoveDown,
    Action::MoveToNextDisplay,
    Action::Jump1,
    Action::Jump2,
    Action::Jump3,
    Action::Jump4,
    Action::Jump5,
    Action::Jump6,
    Action::Jump7,
    Action::Jump8,
    Action::Jump9,
    Action::CycleWidth,
    Action::FullWidth,
    Action::CenterColumn,
    Action::Wider,
    Action::Narrower,
    Action::Taller,
    Action::Shorter,
    Action::EqualizeHeights,
    Action::PinLeft,
    Action::PinRight,
    Action::StackIntoLeft,
    Action::StackIntoRight,
    Action::PopOutLeft,
    Action::PopOutRight,
    Action::ToggleTabs,
    Action::ToggleFloat,
    Action::TogglePause,
    Action::ToggleOverview,
    Action::CycleProfile,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub key_code: u16,
    pub modifiers: Mod,
    pub action: Action,
}

impl Binding {
    pub fn new(key_code: u16, modifiers: Mod, action: Action) -> Self {
        Self { key_code, modifiers, action }
    }

    pub fn defaults() -> Vec<Binding> {
        let hold = Mod::new(Mod::CONTROL | Mod::OPTION);
        let shifted = hold.union(Mod::new(Mod::SHIFT));
        let commanded = hold.union(Mod::new(Mod::COMMAND));
        let mut list = vec![
            Binding::new(123, hold, Action::FocusLeft),
            Binding::new(124, hold, Action::FocusRight),
            Binding::new(125, hold, Action::FocusDown),
            Binding::new(126, hold, Action::FocusUp),
            Binding::new(4, hold, Action::FocusLeft),
            Binding::new(37, hold, Action::FocusRight),
            Binding::new(38, hold, Action::FocusDown),
            Binding::new(40, hold, Action::FocusUp),
            Binding::new(123, shifted, Action::MoveLeft),
            Binding::new(124, shifted, Action::MoveRight),
            Binding::new(125, shifted, Action::MoveDown),
            Binding::new(126, shifted, Action::MoveUp),
            Binding::new(4, shifted, Action::MoveLeft),
            Binding::new(37, shifted, Action::MoveRight),
            Binding::new(38, shifted, Action::MoveDown),
            Binding::new(40, shifted, Action::MoveUp),
            Binding::new(123, commanded, Action::FocusFirst),
            Binding::new(124, commanded, Action::FocusLast),
            Binding::new(45, hold, Action::MoveToNextDisplay),
            Binding::new(1, hold, Action::FocusNextDisplay),
            Binding::new(12, hold, Action::TogglePause),
            Binding::new(15, hold, Action::CycleWidth),
            Binding::new(3, hold, Action::FullWidth),
            Binding::new(8, hold, Action::CenterColumn),
            Binding::new(24, hold, Action::Wider),
            Binding::new(27, hold, Action::Narrower),
            Binding::new(24, shifted, Action::Taller),
            Binding::new(27, shifted, Action::Shorter),
            Binding::new(29, hold, Action::EqualizeHeights),
            Binding::new(35, hold, Action::PinLeft),
            Binding::new(35, shifted, Action::PinRight),
            Binding::new(30, hold, Action::StackIntoLeft),
            Binding::new(33, hold, Action::StackIntoRight),
            Binding::new(30, shifted, Action::PopOutRight),
            Binding::new(33, shifted, Action::PopOutLeft),
            Binding::new(17, shifted, Action::ToggleTabs),
            Binding::new(17, hold, Action::ToggleFloat),
            Binding::new(31, hold, Action::ToggleOverview),
            Binding::new(42, hold, Action::CycleProfile),
        ];
        let numbers = [
            (18, Action::Jump1),
            (19, Action::Jump2),
            (20, Action::Jump3),
            (21, Action::Jump4),
            (23, Action::Jump5),
            (22, Action::Jump6),
            (26, Action::Jump7),
            (28, Action::Jump8),
            (25, Action::Jump9),
        ];
        for (code, action) in numbers {
            list.push(Binding::new(code, hold, action));
        }
        list
    }
}

pub fn key_title(code: u16) -> String {
    let special = match code {
        123 => "←",
        124 => "→",
        125 => "↓",
        126 => "↑",
        36 => "↩",
        48 => "⇥",
        49 => "Space",
        51 => "⌫",
        53 => "Esc",
        24 => "=",
        27 => "-",
        30 => "]",
        33 => "[",
        42 => "\\",
        _ => "",
    };
    if !special.is_empty() {
        return special.to_string();
    }
    const LETTERS: &[(u16, &str)] = &[
        (0, "A"),
        (1, "S"),
        (2, "D"),
        (3, "F"),
        (4, "H"),
        (5, "G"),
        (6, "Z"),
        (7, "X"),
        (8, "C"),
        (9, "V"),
        (11, "B"),
        (12, "Q"),
        (13, "W"),
        (14, "E"),
        (15, "R"),
        (16, "Y"),
        (17, "T"),
        (18, "1"),
        (19, "2"),
        (20, "3"),
        (21, "4"),
        (22, "6"),
        (23, "5"),
        (25, "9"),
        (26, "7"),
        (28, "8"),
        (29, "0"),
        (31, "O"),
        (32, "U"),
        (34, "I"),
        (35, "P"),
        (37, "L"),
        (38, "J"),
        (40, "K"),
        (45, "N"),
        (46, "M"),
    ];
    LETTERS
        .iter()
        .find(|(key, _)| *key == code)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| format!("key {code}"))
}
