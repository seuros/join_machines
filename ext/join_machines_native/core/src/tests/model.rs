//! Random operation streams checked against a naive reference that keeps
//! every accepted record forever.
use super::*;
use std::collections::BTreeSet;

struct Draws(u64);
impl Draws {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    const fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
    fn index(&mut self, bound: usize) -> usize {
        usize::try_from(self.below(u64::try_from(bound).unwrap())).unwrap()
    }
}

/// What the runs did, summed. The four properties are asserted on the way;
/// these say how often each was put to the test.
#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    /// `insert` answers, indexed by `answer_index`.
    answers: [usize; 6],
    /// (a) pairs drained, none of them twice.
    drained: usize,
    /// (b) records accepted, none of them twice.
    accepted: usize,
    /// (c) pairs the keep-everything reference expects; equals `drained`.
    expected: usize,
    /// (d) replays of retired records, every one answered `Late`.
    late_replays: usize,
    retired: usize,
    /// Replays of live records as they are, and of records refused earlier.
    live_replays: usize,
    refused_replays: usize,
}
impl Counts {
    fn add(&mut self, other: Self) {
        for (total, count) in self.answers.iter_mut().zip(other.answers) {
            *total += count;
        }
        self.drained += other.drained;
        self.accepted += other.accepted;
        self.expected += other.expected;
        self.late_replays += other.late_replays;
        self.retired += other.retired;
        self.live_replays += other.live_replays;
        self.refused_replays += other.refused_replays;
    }
}

const fn answer_index(answer: Result<Inserting, Refusal>) -> usize {
    match answer {
        Ok(Inserting::Accepted) => 0,
        Ok(Inserting::Duplicate) => 1,
        Err(Refusal::Mismatch) => 2,
        Err(Refusal::Pending) => 3,
        Err(Refusal::Late) => 4,
        Err(Refusal::Full | Refusal::WatermarkRegressed) => 5,
    }
}

/// Naive reference: remembers every accepted record forever (`true` while
/// the join should still hold it) and compares with wide signed arithmetic.
#[derive(Default)]
struct Model {
    seen: [Vec<(Record, bool)>; 2],
    watermark: [Option<u64>; 2],
    owed: Vec<Pair>,
    /// Records offered under a fresh identity and refused every time so far.
    /// An identity names one record for the whole run and is never reused.
    refused: [Vec<Record>; 2],
    /// Every `(left id, right id)` drained so far.
    drained: BTreeSet<(u64, u64)>,
    counts: Counts,
}

impl Model {
    fn matches(before: u64, after: u64, left: Record, right: Record) -> bool {
        let delta = i128::from(right.timestamp) - i128::from(left.timestamp);
        left.key == right.key && delta >= -i128::from(before) && delta <= i128::from(after)
    }
    fn pick(records: &[Record], draws: &mut Draws) -> Option<Record> {
        (!records.is_empty()).then(|| records[draws.index(records.len())])
    }
    fn records(&self, side: usize, live: bool) -> Vec<Record> {
        let seen = self.seen[side].iter();
        seen.filter(|(_, l)| *l == live).map(|(r, _)| *r).collect()
    }
    /// The record to insert: a `fresh` one under an identity never used
    /// before, a live record replayed as it is or with its key or timestamp
    /// changed, a retired record replayed, or a refused record offered again.
    fn candidate(&self, side: usize, fresh: Record, draws: &mut Draws) -> Record {
        let choice = draws.below(10);
        let held = Self::pick(&self.records(side, true), draws);
        let replay = match choice {
            0 => held,
            1 => held.map(|held| rec(held.id, held.key + 1, held.timestamp)),
            2 => held.map(|held| rec(held.id, held.key, held.timestamp ^ 1)),
            3 => Self::pick(&self.records(side, false), draws),
            4 => Self::pick(&self.refused[side], draws),
            _ => None,
        };
        replay.unwrap_or(fresh)
    }
    fn live(&self, side: usize) -> usize {
        self.seen[side].iter().filter(|(_, live)| *live).count()
    }
    /// Retirable when no admissible (above-watermark, representable) opposite
    /// timestamp is within the record's bounds any more, and the record's own
    /// watermark has reached it.
    fn due(&self, before: u64, after: u64, side: usize, record: Record) -> bool {
        let ahead = if side == 0 { after } else { before };
        let last = (i128::from(record.timestamp) + i128::from(ahead)).min(i128::from(u64::MAX));
        self.watermark[1 - side].is_some_and(|mark| i128::from(mark) >= last)
            && self.watermark[side].is_some_and(|mark| mark >= record.timestamp)
    }
    /// What `insert` must answer: lateness is the record's own side's alone.
    fn expect(&self, side: usize, record: Record, room: usize) -> Result<Inserting, Refusal> {
        let held = self.seen[side]
            .iter()
            .find(|(held, live)| *live && held.id == record.id);
        if let Some((held, _)) = held {
            return if *held == record {
                Ok(Inserting::Duplicate)
            } else {
                Err(Refusal::Mismatch)
            };
        }
        if !self.owed.is_empty() {
            Err(Refusal::Pending)
        } else if self.watermark[side].is_some_and(|mark| record.timestamp <= mark) {
            Err(Refusal::Late)
        } else if self.live(side) == room {
            Err(Refusal::Full)
        } else {
            Ok(Inserting::Accepted)
        }
    }
    /// Record an accepted record and every pair the reference owes for it:
    /// one per matching record ever accepted on the other side, held or not.
    fn accept(&mut self, before: u64, after: u64, side: usize, record: Record) {
        // (b) Accepted once in a lifetime: never while live, never again.
        assert!(self.seen[side].iter().all(|(seen, _)| seen.id != record.id));
        for (other, _) in &self.seen[1 - side] {
            let (left, right) = if side == 0 {
                (record, *other)
            } else {
                (*other, record)
            };
            if Self::matches(before, after, left, right) {
                self.owed.push(Pair {
                    key: record.key,
                    left: left.id,
                    right: right.id,
                });
            }
        }
        self.seen[side].push((record, true));
        self.refused[side].retain(|refused| *refused != record);
        self.counts.accepted += 1;
    }
    fn drained(&mut self, pairs: &[Pair]) {
        for pair in pairs {
            let index = self.owed.iter().position(|owed| owed == pair);
            self.owed
                .swap_remove(index.expect("emitted pair must be owed"));
            // (a) No pair of identities is handed over twice.
            assert!(
                self.drained.insert((pair.left, pair.right)),
                "{pair:?} drained twice"
            );
            self.counts.drained += 1;
        }
    }
    /// (d) Every listed record, replayed, is `Late` and changes nothing.
    fn replay_retired<const L: usize, const R: usize>(
        &mut self,
        join: &Join<L, R>,
        retired: &[Retired],
    ) {
        let mut probe = join.clone();
        for gone in retired {
            assert_eq!(probe.insert(gone.side, gone.record), Err(Refusal::Late));
            self.counts.late_replays += 1;
        }
        assert_eq!(&probe, join);
    }
}

const SIDES: [Side; 2] = [Side::Left, Side::Right];
const L: usize = 5;
const R: usize = 7;

struct Run {
    join: Join<L, R>,
    model: Model,
    draws: Draws,
    before: u64,
    after: u64,
    /// Event time both inputs deliver around.
    now: u64,
    issued: u64,
}

impl Run {
    fn insert(&mut self) {
        let side = self.draws.index(2);
        self.now = self.now.saturating_add(self.draws.below(2));
        let timestamp = self
            .now
            .saturating_sub(4)
            .saturating_add(self.draws.below(12));
        let fresh = rec(self.issued, self.draws.below(3), timestamp);
        let record = self.model.candidate(side, fresh, &mut self.draws);
        let model = &mut self.model;
        let retired = model.records(side, false).contains(&record);
        let expected = model.expect(side, record, [L, R][side]);
        if record == fresh {
            self.issued += 1;
            if expected != Ok(Inserting::Accepted) {
                model.refused[side].push(fresh);
            }
        } else if retired {
            // (d) Retired is final: late as soon as nothing is owed.
            assert!(expected == Err(Refusal::Pending) || expected == Err(Refusal::Late));
            model.counts.late_replays += usize::from(expected == Err(Refusal::Late));
        } else if expected == Ok(Inserting::Duplicate) {
            model.counts.live_replays += 1;
        } else if model.refused[side].contains(&record) {
            model.counts.refused_replays += 1;
        }
        model.counts.answers[answer_index(expected)] += 1;
        let snapshot = self.join.clone();
        assert_eq!(self.join.insert(SIDES[side], record), expected);
        if expected == Ok(Inserting::Accepted) {
            model.accept(self.before, self.after, side, record);
        } else {
            // A refusal or an honoured repeat: nothing changed.
            assert_eq!(self.join, snapshot);
        }
        assert_eq!(self.join.pending_pairs(), model.owed.len());
    }

    fn drain(&mut self) {
        let mut buffer = [Pair::default(); 3];
        let room = self.draws.index(4);
        let written = self.join.drain(&mut buffer[..room]);
        assert_eq!(written, room.min(self.model.owed.len()));
        self.model.drained(&buffer[..written]);
        assert_eq!(self.join.pending(), !self.model.owed.is_empty());
    }

    /// The two watermarks move independently and unevenly on purpose: the
    /// `laggard` is advanced rarely and to a value well behind event time,
    /// the other often and up to slightly ahead of it.
    fn advance(&mut self, laggard: usize) {
        let side = if self.draws.below(5) == 0 {
            laggard
        } else {
            1 - laggard
        };
        let value = if side == laggard {
            self.now.saturating_sub(self.draws.below(25))
        } else {
            self.now
                .saturating_sub(3)
                .saturating_add(self.draws.below(8))
        };
        let mark = self.model.watermark[side];
        let expected = match mark {
            Some(mark) if value < mark => Err(Refusal::WatermarkRegressed),
            Some(mark) if value == mark => Ok(Advancing::Repeated),
            Some(_) | None => Ok(Advancing::Advanced),
        };
        let snapshot = self.join.clone();
        assert_eq!(self.join.advance_watermark(SIDES[side], value), expected);
        if expected == Ok(Advancing::Advanced) {
            self.model.watermark[side] = Some(value);
        } else {
            assert_eq!(self.join, snapshot);
        }
        assert_eq!(self.join.watermark(SIDES[side]), self.model.watermark[side]);
    }

    fn retire(&mut self) {
        let mut buffer = [Retired::default(); 3];
        let room = self.draws.index(4);
        let snapshot = self.join.clone();
        let result = self.join.retire(&mut buffer[..room]);
        let (before, after, model) = (self.before, self.after, &mut self.model);
        if !model.owed.is_empty() {
            assert_eq!(result, Err(Refusal::Pending));
            assert_eq!(self.join, snapshot);
            return;
        }
        let due = |side: usize| {
            let seen = model.seen[side].iter();
            seen.filter(|(held, live)| *live && model.due(before, after, side, *held))
                .count()
        };
        let due = due(0) + due(1);
        assert_eq!(snapshot.retirable(), due);
        assert_eq!(result, Ok(room.min(due)));
        let gone = &buffer[..room.min(due)];
        for gone in gone {
            let side = usize::from(gone.side == Side::Right);
            assert!(model.due(before, after, side, gone.record));
            let entry = model.seen[side]
                .iter_mut()
                .find(|(held, live)| *live && *held == gone.record);
            entry.expect("retired record must be live").1 = false;
        }
        model.counts.retired += gone.len();
        model.replay_retired(&self.join, gone);
    }

    /// Drain what is left, then judge the whole run.
    fn finish(mut self) -> Counts {
        let mut buffer = [Pair::default(); 3];
        while self.join.pending() {
            let written = self.join.drain(&mut buffer);
            self.model.drained(&buffer[..written]);
        }
        let (model, mut reference) = (&mut self.model, BTreeSet::new());
        assert_eq!(model.owed, []);
        // (c) Completeness: every two accepted records within bounds were
        // drained as a pair, exactly once, whenever each of them arrived.
        for (left, _) in &model.seen[0] {
            for (right, _) in &model.seen[1] {
                if Model::matches(self.before, self.after, *left, *right) {
                    reference.insert((left.id, right.id));
                }
            }
        }
        assert_eq!(model.drained, reference);
        assert_eq!(model.counts.drained, reference.len());
        model.counts.expected = reference.len();
        // (d) Every record ever retired is still late at the end of the run.
        for (index, side) in SIDES.into_iter().enumerate() {
            let records = model.records(index, false);
            let gone: Vec<Retired> = records
                .iter()
                .map(|&record| Retired { side, record })
                .collect();
            model.replay_retired(&self.join, &gone);
        }
        model.counts
    }
}

fn run_model(seed: u64, before: u64, after: u64, base: u64) -> Counts {
    let mut run = Run {
        join: Join::new(Bounds::new(before, after)),
        model: Model::default(),
        draws: Draws(seed),
        before,
        after,
        now: base,
        issued: 0,
    };
    for step in 0..4_000 {
        match run.draws.below(10) {
            0..=4 => run.insert(),
            5 | 6 => run.drain(),
            // The lagging input changes sides every 400 steps.
            7 | 8 => run.advance((step / 400) % 2),
            _ => run.retire(),
        }
        for (index, side) in SIDES.into_iter().enumerate() {
            assert_eq!(run.join.len(side), run.model.live(index));
        }
    }
    run.finish()
}

#[test]
fn randomized_runs_match_the_naive_model() {
    let mut counts = Counts::default();
    for seed in 1..=40_u64 {
        let mut draws = Draws(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (before, after) = (draws.below(6), draws.below(6));
        counts.add(run_model(seed, before, after, 0));
        counts.add(run_model(seed + 1_000, before, after, u64::MAX - 400));
    }
    counts.add(run_model(7, u64::MAX, u64::MAX, 0));
    counts.add(run_model(8, 0, 0, 0));
    std::println!("{counts:?}");
    // Every answer of `insert` was exercised (and clone-compared) many times,
    // and so was each kind of replay and each of the four properties.
    assert!(
        counts.answers.iter().all(|count| *count > 100),
        "{counts:?}"
    );
    assert_eq!(counts.drained, counts.expected);
    let tested = [
        counts.drained,
        counts.accepted,
        counts.retired,
        counts.late_replays,
    ];
    assert!(tested.iter().all(|count| *count > 1_000), "{counts:?}");
    let replays = [counts.live_replays, counts.refused_replays];
    assert!(replays.iter().all(|count| *count > 1_000), "{counts:?}");
}
