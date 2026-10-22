use super::model::{PinEdge, VerticalAlign};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrange {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    LeftThird,
    CenterThird,
    RightThird,
    LeftTwoThirds,
    RightTwoThirds,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Full,
    Columns2,
    Columns3,
    Columns4,
    LeftAndTwoRight,
    TwoLeftAndRight,
    Quarters,
    TopAndBottom,
    WideLeftTwoRight,
    TwoLeftWideRight,
    Sidebars,
    ThirdAndWide,
    WideAndThird,
    Stacked3,
    Six,
}

#[derive(Clone, Copy, Debug)]
pub struct Pane {
    pub fraction: f64,
    pub height: f64,
    pub vertical: VerticalAlign,
    pub pin: Option<PinEdge>,
    pub center: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Stack {
    pub share: f64,
    pub count: usize,
    pub weights: &'static [f64],
}

pub struct Glyph {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub primary: bool,
}

impl Arrange {
    pub fn all() -> &'static [Arrange] {
        &ALL
    }

    pub fn from_index(index: isize) -> Option<Arrange> {
        ALL.get(index as usize).copied()
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::LeftHalf => "Left Half",
            Self::RightHalf => "Right Half",
            Self::TopHalf => "Top Half",
            Self::BottomHalf => "Bottom Half",
            Self::LeftThird => "Left Third",
            Self::CenterThird => "Center Third",
            Self::RightThird => "Right Third",
            Self::LeftTwoThirds => "Left Two Thirds",
            Self::RightTwoThirds => "Right Two Thirds",
            Self::TopLeft => "Top Left",
            Self::TopRight => "Top Right",
            Self::BottomLeft => "Bottom Left",
            Self::BottomRight => "Bottom Right",
            Self::Full => "Full Screen",
            Self::Columns2 => "Two Columns",
            Self::Columns3 => "Three Columns",
            Self::Columns4 => "Four Columns",
            Self::LeftAndTwoRight => "Left and Two on the Right",
            Self::TwoLeftAndRight => "Two on the Left and Right",
            Self::Quarters => "Four Quarters",
            Self::TopAndBottom => "Top and Bottom",
            Self::WideLeftTwoRight => "Wide Left and Two on the Right",
            Self::TwoLeftWideRight => "Two on the Left and Wide Right",
            Self::Sidebars => "Center and Sidebars",
            Self::ThirdAndWide => "Third and Wide Right",
            Self::WideAndThird => "Wide Left and Third",
            Self::Stacked3 => "Three Stacked",
            Self::Six => "Six Windows",
        }
    }

    pub fn glyphs(self) -> Vec<Glyph> {
        if let Some(pane) = self.pane() {
            return vec![pane_glyph(pane)];
        }
        self.stacks().map(stack_glyphs).unwrap_or_default()
    }

    pub fn section(self) -> &'static str {
        if self.stacks().is_some() {
            "Fill & Arrange"
        } else {
            "Move & Resize"
        }
    }

    pub fn pane(self) -> Option<Pane> {
        let top = VerticalAlign::Top;
        let bottom = VerticalAlign::Bottom;
        Some(match self {
            Self::LeftHalf => pane(0.5, 1.0, top, None, false),
            Self::RightHalf => pane(0.5, 1.0, top, Some(PinEdge::Right), false),
            Self::TopHalf => pane(1.0, 0.5, top, None, false),
            Self::BottomHalf => pane(1.0, 0.5, bottom, None, false),
            Self::LeftThird => pane(1.0 / 3.0, 1.0, top, None, false),
            Self::CenterThird => pane(1.0 / 3.0, 1.0, top, None, true),
            Self::RightThird => pane(1.0 / 3.0, 1.0, top, Some(PinEdge::Right), false),
            Self::LeftTwoThirds => pane(2.0 / 3.0, 1.0, top, None, false),
            Self::RightTwoThirds => pane(2.0 / 3.0, 1.0, top, Some(PinEdge::Right), false),
            Self::TopLeft => pane(0.5, 0.5, top, None, false),
            Self::TopRight => pane(0.5, 0.5, top, Some(PinEdge::Right), false),
            Self::BottomLeft => pane(0.5, 0.5, bottom, None, false),
            Self::BottomRight => pane(0.5, 0.5, bottom, Some(PinEdge::Right), false),
            Self::Full => pane(1.0, 1.0, top, None, false),
            _ => return None,
        })
    }

    pub fn stacks(self) -> Option<&'static [Stack]> {
        Some(match self {
            Self::Columns2 => &TWO,
            Self::Columns3 => &THREE,
            Self::Columns4 => &FOUR,
            Self::LeftAndTwoRight => &LEFT_TWO_RIGHT,
            Self::TwoLeftAndRight => &TWO_LEFT_RIGHT,
            Self::Quarters => &QUARTERS,
            Self::TopAndBottom => &TOP_BOTTOM,
            Self::WideLeftTwoRight => &WIDE_LEFT_TWO,
            Self::TwoLeftWideRight => &TWO_WIDE_RIGHT,
            Self::Sidebars => &SIDEBARS,
            Self::ThirdAndWide => &THIRD_WIDE,
            Self::WideAndThird => &WIDE_THIRD,
            Self::Stacked3 => &STACKED3,
            Self::Six => &SIX,
            _ => return None,
        })
    }
}

const fn pane(fraction: f64, height: f64, vertical: VerticalAlign, pin: Option<PinEdge>, center: bool) -> Pane {
    Pane { fraction, height, vertical, pin, center }
}

fn pane_glyph(pane: Pane) -> Glyph {
    let w = pane.fraction.clamp(0.2, 1.0);
    let h = pane.height.clamp(0.2, 1.0);
    let x = if pane.pin == Some(PinEdge::Right) {
        1.0 - w
    } else if pane.center {
        (1.0 - w) / 2.0
    } else {
        0.0
    };
    let y = match pane.vertical {
        VerticalAlign::Bottom => 1.0 - h,
        VerticalAlign::Center => (1.0 - h) / 2.0,
        VerticalAlign::Top => 0.0,
    };
    Glyph { x, y, w, h, primary: true }
}

fn stack_glyphs(stacks: &[Stack]) -> Vec<Glyph> {
    let total = stacks.iter().map(|stack| stack.share).sum::<f64>().max(0.01);
    let mut glyphs = Vec::new();
    let mut x = 0.0;
    let mut primary = true;
    for stack in stacks {
        let width = stack.share / total;
        let rows = stack.count.max(1);
        let height = 1.0 / rows as f64;
        for row in 0..rows {
            glyphs.push(Glyph {
                x,
                y: row as f64 * height,
                w: width,
                h: height,
                primary,
            });
            primary = false;
        }
        x += width;
    }
    glyphs
}

const fn stack(share: f64, count: usize) -> Stack {
    Stack { share, count, weights: &[] }
}

const TWO: [Stack; 2] = [stack(1.0, 1), stack(1.0, 1)];
const THREE: [Stack; 3] = [stack(1.0, 1), stack(1.0, 1), stack(1.0, 1)];
const FOUR: [Stack; 4] = [stack(1.0, 1), stack(1.0, 1), stack(1.0, 1), stack(1.0, 1)];
const LEFT_TWO_RIGHT: [Stack; 2] = [stack(1.0, 1), stack(1.0, 2)];
const TWO_LEFT_RIGHT: [Stack; 2] = [stack(1.0, 2), stack(1.0, 1)];
const QUARTERS: [Stack; 2] = [stack(1.0, 2), stack(1.0, 2)];
const TOP_BOTTOM: [Stack; 1] = [stack(1.0, 2)];
const WIDE_LEFT_TWO: [Stack; 2] = [stack(2.0, 1), stack(1.0, 2)];
const TWO_WIDE_RIGHT: [Stack; 2] = [stack(1.0, 2), stack(2.0, 1)];
const SIDEBARS: [Stack; 3] = [stack(1.0, 1), stack(2.0, 1), stack(1.0, 1)];
const THIRD_WIDE: [Stack; 2] = [stack(1.0, 1), stack(2.0, 1)];
const WIDE_THIRD: [Stack; 2] = [stack(2.0, 1), stack(1.0, 1)];
const STACKED3: [Stack; 1] = [stack(1.0, 3)];
const SIX: [Stack; 3] = [stack(1.0, 2), stack(1.0, 2), stack(1.0, 2)];

const ALL: [Arrange; 28] = [
    Arrange::LeftHalf,
    Arrange::RightHalf,
    Arrange::TopHalf,
    Arrange::BottomHalf,
    Arrange::LeftThird,
    Arrange::CenterThird,
    Arrange::RightThird,
    Arrange::LeftTwoThirds,
    Arrange::RightTwoThirds,
    Arrange::TopLeft,
    Arrange::TopRight,
    Arrange::BottomLeft,
    Arrange::BottomRight,
    Arrange::Full,
    Arrange::Columns2,
    Arrange::Columns3,
    Arrange::Columns4,
    Arrange::LeftAndTwoRight,
    Arrange::TwoLeftAndRight,
    Arrange::Quarters,
    Arrange::TopAndBottom,
    Arrange::WideLeftTwoRight,
    Arrange::TwoLeftWideRight,
    Arrange::Sidebars,
    Arrange::ThirdAndWide,
    Arrange::WideAndThird,
    Arrange::Stacked3,
    Arrange::Six,
];

pub fn take_groups<T: Copy>(stacks: &[Stack], windows: &[T]) -> (Vec<(Stack, Vec<T>)>, Vec<T>) {
    let mut index = 0;
    let mut groups = Vec::new();
    for stack in stacks {
        if index >= windows.len() {
            break;
        }
        let end = (index + stack.count).min(windows.len());
        groups.push((*stack, windows[index..end].to_vec()));
        index = end;
    }
    (groups, windows[index..].to_vec())
}

pub fn ratios(shares: &[f64], gap: f64, viewport: f64) -> Vec<f64> {
    if shares.is_empty() {
        return Vec::new();
    }
    let gaps = gap * shares.len().saturating_sub(1) as f64;
    let inner = (viewport - gaps).max(80.0);
    let total = shares.iter().sum::<f64>().max(0.01);
    shares.iter().map(|share| (share / total) * inner / viewport.max(1.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarters_fill_two_stacks_and_keep_the_rest() {
        let windows = [1, 2, 3, 4, 5];
        let (groups, extra) = take_groups(Arrange::Quarters.stacks().unwrap(), &windows);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].1, vec![1, 2]);
        assert_eq!(groups[1].1, vec![3, 4]);
        assert_eq!(extra, vec![5]);
    }

    #[test]
    fn icons_show_where_the_windows_go() {
        let left = Arrange::LeftHalf.glyphs();
        assert_eq!(left.len(), 1);
        assert!(left[0].x < 0.01);
        let right = Arrange::RightHalf.glyphs();
        assert!(right[0].x > 0.4);
        let bottom = Arrange::BottomHalf.glyphs();
        assert!(bottom[0].y > 0.4);
        assert_eq!(Arrange::Quarters.glyphs().len(), 4);
        assert_eq!(Arrange::Six.glyphs().len(), 6);
        assert!(Arrange::Columns2.glyphs().iter().filter(|glyph| glyph.primary).count() == 1);
    }

    #[test]
    fn two_windows_in_a_four_column_layout_share_the_screen() {
        let ratios = ratios(&[1.0, 1.0], 8.0, 1400.0);
        let width: f64 = ratios.iter().map(|ratio| ratio * 1400.0).sum();
        assert!((width - (1400.0 - 8.0)).abs() < 0.1);
    }
}
