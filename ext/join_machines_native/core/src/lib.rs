#![no_std]
#![doc = include_str!("../README.md")]
/// Per-input event-time progress, the sibling type this join is built on, and
/// what an honoured advance of it did: `Advanced`, or `Repeated` for an
/// idempotent repeat that changed nothing. Both are honoured; neither is a
/// refusal.
pub use window_machines::{Advancing, Watermark};

mod refusal;

pub use refusal::Refusal;

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

/// Inclusive range of opposite-side event times a record can match, never
/// empty: `first <= last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    first: u64,
    last: u64,
}
impl Reach {
    /// The range from `first` to `last` inclusive, or `None` unless
    /// `first <= last`: a range holding no timestamp is not a reach.
    #[must_use]
    pub const fn new(first: u64, last: u64) -> Option<Self> {
        if first <= last {
            Some(Self { first, last })
        } else {
            None
        }
    }

    /// Earliest matching timestamp.
    #[must_use]
    pub const fn first(self) -> u64 {
        self.first
    }

    /// Latest matching timestamp.
    #[must_use]
    pub const fn last(self) -> u64 {
        self.last
    }

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
        // Saturating either way keeps `first <= timestamp <= last`.
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

/// What an honoured [`Join::insert`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserting {
    /// The record is now held and its pairs are owed.
    Accepted,
    /// An idempotent repeat: this exact record (same side, identity, key and
    /// timestamp) is already held. Nothing changed and no pair is owed twice.
    Duplicate,
}

/// The pair owed next: `record` on `side` against `other`, the held opposite
/// record in slot `cursor`. It carries `other` so draining never looks the
/// slot up again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Probe {
    side: Side,
    record: Record,
    cursor: usize,
    other: Record,
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
        self.records(side).count()
    }

    /// Whether neither input holds a record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len(Side::Left) == 0 && self.len(Side::Right) == 0
    }

    /// Whether a live record on `side` uses this identity.
    #[must_use]
    pub fn contains(&self, side: Side, id: u64) -> bool {
        self.held(side, id).is_some()
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
    /// A record whose identity is already held on `side` is answered from the
    /// held record, before anything else is checked: the same key and
    /// timestamp is the effect asked for already holding,
    /// [`Inserting::Duplicate`], and nothing changes (no pair is owed twice);
    /// any other contents are [`Refusal::Mismatch`]. Held means live right now:
    /// a matched and drained record is still held, a retired one is not.
    ///
    /// A record that is not held is late when its timestamp is at or below
    /// its own side's watermark, and on no other count: the opposite
    /// watermark never refuses a record, so one whose partner is still held
    /// is always admitted and paired, however far the two watermarks have
    /// drifted apart. A record replayed after it was retired is
    /// [`Refusal::Late`] for good, because [`retire`](Self::retire) lets a
    /// record go only once its own watermark has reached it.
    ///
    /// # Errors
    /// [`Refusal::Mismatch`] for a held identity with other contents,
    /// [`Refusal::Pending`] while earlier pairs are undrained, [`Refusal::Late`]
    /// at or below the side's own watermark, [`Refusal::Full`] without a free
    /// slot. Nothing changes.
    pub fn insert(&mut self, side: Side, record: Record) -> Result<Inserting, Refusal> {
        if let Some(held) = self.held(side, record.id) {
            return if held == record {
                Ok(Inserting::Duplicate)
            } else {
                Err(Refusal::Mismatch)
            };
        }
        if self.probe.is_some() {
            return Err(Refusal::Pending);
        }
        if self.passed(side, record.timestamp) {
            return Err(Refusal::Late);
        }
        let slot = self
            .slots(side)
            .iter()
            .position(Option::is_none)
            .ok_or(Refusal::Full)?;
        self.slots_mut(side)[slot] = Some(record);
        self.probe = self.settle(side, record, 0);
        Ok(Inserting::Accepted)
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
            probe = self.next(current);
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
            let other = probe.other;
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
            self.probe = self.next(probe);
        }
        written
    }

    /// Advance or repeat one input's event-time progress.
    ///
    /// [`Advancing::Advanced`] when the side moved forward (or took its first
    /// value). Repeating the value the side already holds is the effect asked
    /// for already holding: [`Advancing::Repeated`], and nothing changes.
    /// Advancing never removes records; call [`retire`](Self::retire) for that.
    ///
    /// # Errors
    /// [`Refusal::WatermarkRegressed`] when `value` is behind the side's
    /// watermark. Nothing changes.
    pub const fn advance_watermark(
        &mut self,
        side: Side,
        value: u64,
    ) -> Result<Advancing, Refusal> {
        // Asks the sibling where the side stands, then acts: the sibling's
        // `advance` would answer refusals only its windows and sessions give.
        let advancing = match self.watermark(side) {
            Some(held) if value < held => return Err(Refusal::WatermarkRegressed),
            Some(held) if value == held => return Ok(Advancing::Repeated),
            Some(_) | None => Advancing::Advanced,
        };
        match side {
            Side::Left => self.left_watermark = Watermark::at(value),
            Side::Right => self.right_watermark = Watermark::at(value),
        }
        Ok(advancing)
    }

    /// Advance or repeat one input's progress from a caller-owned [`Watermark`].
    ///
    /// Honoured exactly when the side then equals `source`. A `source` with a
    /// value behaves as [`advance_watermark`](Self::advance_watermark) with
    /// that value. A `source` with no value yet has nothing to report: while
    /// the side is uninitialized too they already agree, which is
    /// [`Advancing::Repeated`]. It never clears a side.
    ///
    /// # Errors
    /// [`Refusal::WatermarkRegressed`] when `source` is behind the side: its
    /// value is lower, or it has none while the side holds one. Nothing changes.
    pub const fn advance_watermark_from(
        &mut self,
        side: Side,
        source: &Watermark,
    ) -> Result<Advancing, Refusal> {
        match (source.value(), self.watermark(side)) {
            (Some(value), Some(_) | None) => self.advance_watermark(side, value),
            (None, None) => Ok(Advancing::Repeated),
            (None, Some(_)) => Err(Refusal::WatermarkRegressed),
        }
    }

    /// Held records [`retire`](Self::retire) would remove right now: those
    /// both watermarks have finished.
    #[must_use]
    pub fn retirable(&self) -> usize {
        [Side::Left, Side::Right]
            .into_iter()
            .map(|side| {
                self.records(side)
                    .filter(|held| self.finished(side, held.timestamp))
                    .count()
            })
            .sum()
    }

    /// Remove the records both watermarks have finished; returns how many.
    ///
    /// A held record is finished when both hold: the opposite watermark has
    /// reached the end of its reach (`timestamp + after` for a left record,
    /// `timestamp + before` for a right one, saturating at `u64::MAX`), so no
    /// opposite record [`insert`](Self::insert) would still accept can match
    /// it; and its own side's watermark has reached its timestamp, so the same
    /// record offered again is [`Refusal::Late`]. Hence no pair is ever owed
    /// twice and no record that could still match is dropped. A record whose
    /// own watermark lags stays held, whatever the opposite one says.
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
                if !self.finished(side, record.timestamp) {
                    continue;
                }
                self.slots_mut(side)[index] = None;
                *slot = Retired { side, record };
                written += 1;
            }
        }
        Ok(written)
    }

    /// The live record on `side` using this identity.
    fn held(&self, side: Side, id: u64) -> Option<Record> {
        self.records(side).copied().find(|held| held.id == id)
    }

    /// The live records on `side`, in slot order.
    fn records(&self, side: Side) -> impl Iterator<Item = &Record> {
        self.slots(side).iter().flatten()
    }

    const fn slots(&self, side: Side) -> &[Option<Record>] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    const fn slots_mut(&mut self, side: Side) -> &mut [Option<Record>] {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// Whether `side`'s own watermark has reached `timestamp`: a record there
    /// that is not held is [`Refusal::Late`].
    ///
    /// The one lateness test. `insert` refuses by it and `finished` requires
    /// it, so "retired" implies "late on replay" by construction, and for
    /// good: an honoured advance never lowers a watermark.
    fn passed(&self, side: Side, timestamp: u64) -> bool {
        self.watermark(side).is_some_and(|own| timestamp <= own)
    }

    /// Whether a held `side` record at `timestamp` may be retired. Both hold:
    ///
    /// 1. The opposite watermark has reached the end of the record's reach,
    ///    `other >= reach.last`. An opposite record is accepted only above its
    ///    own watermark, `u > other >= reach.last`, which is outside the
    ///    reach, so no future opposite record matches. One short
    ///    (`other == reach.last - 1`) a record at `reach.last` is still
    ///    accepted and matches, so the test is exact. A reach that saturates
    ///    at `u64::MAX` is reached only by `other == u64::MAX`, where no
    ///    opposite record is accepted at all.
    /// 2. The record's own watermark has reached it ([`passed`](Self::passed)),
    ///    so the same record offered again is `Late`.
    ///
    /// The one retirement test, used by `retire` and `retirable`.
    fn finished(&self, side: Side, timestamp: u64) -> bool {
        let last = self.bounds.reach(side, timestamp).last;
        let unmatchable = self
            .watermark(side.other())
            .is_some_and(|other| other >= last);
        unmatchable && self.passed(side, timestamp)
    }

    /// The first pair `record` on `side` owes from opposite slot `from` on,
    /// or `None` when it owes no more.
    fn settle(&self, side: Side, record: Record, from: usize) -> Option<Probe> {
        let reach = self.bounds.reach(side, record.timestamp);
        self.slots(side.other())
            .iter()
            .enumerate()
            .skip(from)
            .find_map(|(cursor, held)| {
                held.filter(|held| held.key == record.key && reach.contains(held.timestamp))
                    .map(|other| Probe {
                        side,
                        record,
                        cursor,
                        other,
                    })
            })
    }

    /// The pair owed after `probe`.
    fn next(&self, probe: Probe) -> Option<Probe> {
        self.settle(probe.side, probe.record, probe.cursor + 1)
    }
}

#[cfg(test)]
mod tests;
