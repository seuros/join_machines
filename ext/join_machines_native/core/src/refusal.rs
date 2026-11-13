//! The one refusal type of the crate.

use core::fmt;

/// Why a call changed nothing.
///
/// [`Join::insert`](crate::Join::insert) answers [`Mismatch`](Self::Mismatch),
/// [`Pending`](Self::Pending), [`Late`](Self::Late) or [`Full`](Self::Full);
/// [`Join::retire`](crate::Join::retire) answers [`Pending`](Self::Pending);
/// [`Join::advance_watermark`](crate::Join::advance_watermark) and
/// [`Join::advance_watermark_from`](crate::Join::advance_watermark_from) answer
/// [`WatermarkRegressed`](Self::WatermarkRegressed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Pairs from the previous insert are still owed; drain them first.
    Pending,
    /// The record's timestamp is at or below its own side's watermark. The
    /// opposite watermark never makes a record late.
    Late,
    /// A held record on that side uses this identity with another key or
    /// timestamp. (The same contents are an honoured repeat, not a refusal.)
    Mismatch,
    /// That side has no free slot; advance watermarks and retire.
    Full,
    /// The watermark is behind the one that side already holds.
    WatermarkRegressed,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pending => "pairs from the previous insert are still owed",
            Self::Late => "record is at or below its side's watermark",
            Self::Mismatch => "a held record on that side uses this identity with other contents",
            Self::Full => "that side has no free slot",
            Self::WatermarkRegressed => "watermark is behind the one the side holds",
        })
    }
}

impl core::error::Error for Refusal {}
