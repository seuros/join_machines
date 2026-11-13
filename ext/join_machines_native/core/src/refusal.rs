//! The one refusal type of the crate.

use core::fmt;

/// Why a call changed nothing.
///
/// [`Join::insert`](crate::Join::insert) answers [`Pending`](Self::Pending),
/// [`Late`](Self::Late), [`Duplicate`](Self::Duplicate) or [`Full`](Self::Full);
/// [`Join::retire`](crate::Join::retire) answers [`Pending`](Self::Pending);
/// [`Join::advance_watermark`](crate::Join::advance_watermark) and
/// [`Join::advance_watermark_from`](crate::Join::advance_watermark_from) answer
/// [`WatermarkRegressed`](Self::WatermarkRegressed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Pairs from the previous insert are still owed; drain them first.
    Pending,
    /// The timestamp is at or below its own side's watermark.
    Late,
    /// A live record on that side already uses this identity.
    Duplicate,
    /// That side has no free slot; advance watermarks and retire.
    Full,
    /// The watermark is behind the one that side already holds.
    WatermarkRegressed,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pending => "pairs from the previous insert are still owed",
            Self::Late => "timestamp is at or below the side's watermark",
            Self::Duplicate => "a live record on that side already uses this identity",
            Self::Full => "that side has no free slot",
            Self::WatermarkRegressed => "watermark is behind the one the side holds",
        })
    }
}

impl core::error::Error for Refusal {}
