#![no_std]
#![doc = include_str!("../README.md")]
/// Per-input event-time progress, the sibling type this join is built on.
pub use window_machines::Watermark;

/// One of the two join inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The input whose timestamp anchors the interval.
    Left,
    /// The input matched relative to a left timestamp.
    Right,
}
impl Side {
    /// The opposite input.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Join metadata for one externally stored record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    /// Caller-assigned identity, unique among live records of its side.
    pub id: u64,
    /// Equi-join key; hashing a wider key into it is the caller's job.
    pub key: u64,
    /// Event time in caller-defined units.
    pub timestamp: u64,
}

/// Inclusive range of opposite-side event times a record can match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// Earliest matching timestamp.
    pub first: u64,
    /// Latest matching timestamp.
    pub last: u64,
}
impl Reach {
    /// Whether an opposite-side timestamp is inside the range.
    #[must_use]
    pub const fn contains(self, timestamp: u64) -> bool {
        self.first <= timestamp && timestamp <= self.last
    }
}

/// Interval bounds: a pair matches when
/// `left - before <= right <= left + after` in event time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    before: u64,
    after: u64,
}
impl Bounds {
    /// Right records may trail a left record by `before` and lead it by `after`.
    #[must_use]
    pub const fn new(before: u64, after: u64) -> Self {
        Self { before, after }
    }
    /// How far a right record may be behind its left record.
    #[must_use]
    pub const fn before(self) -> u64 {
        self.before
    }
    /// How far a right record may be ahead of its left record.
    #[must_use]
    pub const fn after(self) -> u64 {
        self.after
    }
    /// Opposite-side timestamps reachable from a record on `side`.
    ///
    /// Endpoints saturate at the `u64` domain edges, which is exact: no
    /// timestamp exists beyond them.
    #[must_use]
    pub const fn reach(self, side: Side, timestamp: u64) -> Reach {
        let (back, ahead) = match side {
            Side::Left => (self.before, self.after),
            Side::Right => (self.after, self.before),
        };
        Reach {
            first: timestamp.saturating_sub(back),
            last: timestamp.saturating_add(ahead),
        }
    }
}

/// A matched pair of record identities sharing `key`.
///
/// The `Default` value is an output-buffer placeholder (`[Pair::default(); N]`).
/// It is not a sentinel: a real pair of zero-valued records equals it, so read
/// only the entries the call reports.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    /// The shared join key.
    pub key: u64,
    /// Left record identity.
    pub left: u64,
    /// Right record identity.
    pub right: u64,
}

/// A record dropped from the join; its external payload may be released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retired {
    /// Input the record belonged to.
    pub side: Side,
    /// The metadata that was held.
    pub record: Record,
}
/// An output-buffer placeholder (`[Retired::default(); N]`): a zeroed left
/// record. It is not a sentinel: a real retired record can equal it, so read
/// only the entries the call reports.
impl Default for Retired {
    fn default() -> Self {
        Self {
            side: Side::Left,
            record: Record {
                id: 0,
                key: 0,
                timestamp: 0,
            },
        }
    }
}

/// Why a call changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// Pairs from the previous insert are still owed; drain them first.
    Pending,
    /// The timestamp is at or below its own side's watermark.
    Late,
    /// A live record on that side already uses this identity.
    Duplicate,
    /// That side has no free slot; advance watermarks and retire.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Probe {
    side: Side,
    record: Record,
    cursor: usize,
}

/// Bounded two-input interval join over record metadata.
///
/// `L` and `R` are the live-record capacities of the left and right inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Join<const L: usize, const R: usize> {
    bounds: Bounds,
    left: [Option<Record>; L],
    right: [Option<Record>; R],
    left_watermark: Watermark,
    right_watermark: Watermark,
    probe: Option<Probe>,
}

impl<const L: usize, const R: usize> Join<L, R> {
    /// Construct an empty join with uninitialized watermarks.
    #[must_use]
    pub fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            left: [None; L],
            right: [None; R],
            left_watermark: Watermark::default(),
            right_watermark: Watermark::default(),
            probe: None,
        }
    }

    /// The interval bounds fixed at construction.
    #[must_use]
    pub const fn bounds(&self) -> Bounds {
        self.bounds
    }

    /// Live-record capacity of one input.
    #[must_use]
    pub const fn capacity(&self, side: Side) -> usize {
        match side {
            Side::Left => L,
            Side::Right => R,
        }
    }

    /// Live records currently held for one input.
    #[must_use]
    pub fn len(&self, side: Side) -> usize {
        self.slots(side).iter().flatten().count()
    }

    /// Whether neither input holds a record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len(Side::Left) == 0 && self.len(Side::Right) == 0
    }

    /// Whether a live record on `side` uses this identity.
    #[must_use]
    pub fn contains(&self, side: Side, id: u64) -> bool {
        self.slots(side).iter().flatten().any(|held| held.id == id)
    }

    /// Last accepted watermark of one input, or `None` before the first.
    #[must_use]
    pub const fn watermark(&self, side: Side) -> Option<u64> {
        self.side_watermark(side).value()
    }

    /// One input's event-time progress as the sibling [`Watermark`], a copy.
    ///
    /// Same state [`watermark`](Self::watermark) reads as an integer; use it to
    /// ask sibling questions such as [`Watermark::closes`].
    #[must_use]
    pub const fn side_watermark(&self, side: Side) -> Watermark {
        match side {
            Side::Left => self.left_watermark,
            Side::Right => self.right_watermark,
        }
    }

    /// Admit a record and owe every pair it forms with held opposite records.
    ///
    /// # Errors
    /// [`Refusal::Pending`] while earlier pairs are undrained, [`Refusal::Late`]
    /// at or below the side's watermark, [`Refusal::Duplicate`] for a live
    /// identity, [`Refusal::Full`] without a free slot. Nothing changes.
    pub fn insert(&mut self, side: Side, record: Record) -> Result<(), Refusal> {
        if self.probe.is_some() {
            return Err(Refusal::Pending);
        }
        if self
            .watermark(side)
            .is_some_and(|value| record.timestamp <= value)
        {
            return Err(Refusal::Late);
        }
        if self.contains(side, record.id) {
            return Err(Refusal::Duplicate);
        }
        let slot = self
            .slots(side)
            .iter()
            .position(Option::is_none)
            .ok_or(Refusal::Full)?;
        match side {
            Side::Left => self.left[slot] = Some(record),
            Side::Right => self.right[slot] = Some(record),
        }
        self.probe = self.settle(Probe {
            side,
            record,
            cursor: 0,
        });
        Ok(())
    }

    /// Whether at least one pair from the last insert is still owed.
    #[must_use]
    pub const fn pending(&self) -> bool {
        self.probe.is_some()
    }

    /// Number of pairs still owed from the last insert.
    #[must_use]
    pub fn pending_pairs(&self) -> usize {
        let mut count = 0;
        let mut probe = self.probe;
        while let Some(current) = probe {
            count += 1;
            probe = self.settle(Probe {
                cursor: current.cursor + 1,
                ..current
            });
        }
        count
    }

    /// Move owed pairs into `out`, each exactly once; returns how many.
    ///
    /// A full buffer loses nothing: the join stays [`pending`](Self::pending)
    /// and the next call resumes after the last pair written.
    pub fn drain(&mut self, out: &mut [Pair]) -> usize {
        let mut written = 0;
        for slot in out {
            let Some(probe) = self.probe else {
                break;
            };
            let Some(other) = self
                .slots(probe.side.other())
                .get(probe.cursor)
                .copied()
                .flatten()
            else {
                // Unreachable: a settled cursor always names a matching slot.
                self.probe = None;
                break;
            };
            *slot = match probe.side {
                Side::Left => Pair {
                    key: probe.record.key,
                    left: probe.record.id,
                    right: other.id,
                },
                Side::Right => Pair {
                    key: probe.record.key,
                    left: other.id,
                    right: probe.record.id,
                },
            };
            written += 1;
            self.probe = self.settle(Probe {
                cursor: probe.cursor + 1,
                ..probe
            });
        }
        written
    }

    /// Advance or repeat one input's event-time progress.
    ///
    /// Returns `false` and changes nothing when `value` regresses. Advancing
    /// never removes records; call [`retire`](Self::retire) for that.
    #[must_use]
    pub fn advance_watermark(&mut self, side: Side, value: u64) -> bool {
        match side {
            Side::Left => self.left_watermark.advance(value),
            Side::Right => self.right_watermark.advance(value),
        }
    }

    /// Advance or repeat one input's progress from a caller-owned [`Watermark`].
    ///
    /// Returns `true` exactly when the side now equals `source`. A `source`
    /// with a value behaves as [`advance_watermark`](Self::advance_watermark)
    /// with that value: a regression returns `false` and changes nothing. A
    /// `source` with no value yet has nothing to report: it is an accepted
    /// no-op while the side is uninitialized too, and refused as a regression
    /// once the side holds a value. It never clears a side.
    #[must_use]
    pub fn advance_watermark_from(&mut self, side: Side, source: &Watermark) -> bool {
        match source.value() {
            Some(value) => self.advance_watermark(side, value),
            None => self.watermark(side).is_none(),
        }
    }

    /// Records the opposite watermark has moved past, removable right now.
    #[must_use]
    pub fn retirable(&self) -> usize {
        [Side::Left, Side::Right]
            .into_iter()
            .map(|side| {
                self.slots(side)
                    .iter()
                    .flatten()
                    .filter(|held| self.expired(side, **held))
                    .count()
            })
            .sum()
    }

    /// Remove records no future opposite record can match; returns how many.
    ///
    /// At most `out.len()` records leave per call, left input first; the rest
    /// stay held and [`retirable`](Self::retirable) until a later call.
    ///
    /// # Errors
    /// [`Refusal::Pending`] while pairs are owed, so no owed pair can lose a
    /// partner. Nothing changes.
    pub fn retire(&mut self, out: &mut [Retired]) -> Result<usize, Refusal> {
        if self.probe.is_some() {
            return Err(Refusal::Pending);
        }
        let mut written = 0;
        for side in [Side::Left, Side::Right] {
            for index in 0..self.capacity(side) {
                let Some(slot) = out.get_mut(written) else {
                    return Ok(written);
                };
                let Some(record) = self.slots(side).get(index).copied().flatten() else {
                    continue;
                };
                if !self.expired(side, record) {
                    continue;
                }
                match side {
                    Side::Left => self.left[index] = None,
                    Side::Right => self.right[index] = None,
                }
                *slot = Retired { side, record };
                written += 1;
            }
        }
        Ok(written)
    }

    const fn slots(&self, side: Side) -> &[Option<Record>] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    /// Opposite records at or below their watermark are refused as late, so a
    /// record is dead once that watermark reaches the end of its reach.
    fn expired(&self, side: Side, record: Record) -> bool {
        let last = self.bounds.reach(side, record.timestamp).last;
        self.watermark(side.other())
            .is_some_and(|value| value >= last)
    }

    /// Move the cursor to the next matching opposite slot, or finish.
    fn settle(&self, probe: Probe) -> Option<Probe> {
        let reach = self.bounds.reach(probe.side, probe.record.timestamp);
        self.slots(probe.side.other())
            .iter()
            .enumerate()
            .skip(probe.cursor)
            .find(|(_, held)| {
                held.is_some_and(|held| {
                    held.key == probe.record.key && reach.contains(held.timestamp)
                })
            })
            .map(|(cursor, _)| Probe { cursor, ..probe })
    }
}

#[cfg(test)]
mod tests;
