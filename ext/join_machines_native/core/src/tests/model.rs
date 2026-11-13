//! Random operation streams checked against a naive model.
use super::*;

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
}

/// Naive model: remembers every accepted record forever and compares with
/// wide signed arithmetic, so it shares no reach or retirement logic.
#[derive(Default)]
struct Model {
    seen: [Vec<(Record, bool)>; 2],
    watermark: [Option<u64>; 2],
    owed: Vec<Pair>,
}
impl Model {
    fn matches(before: u64, after: u64, left: Record, right: Record) -> bool {
        let delta = i128::from(right.timestamp) - i128::from(left.timestamp);
        left.key == right.key && delta >= -i128::from(before) && delta <= i128::from(after)
    }
    fn live(&self, side: usize) -> usize {
        self.seen[side].iter().filter(|(_, live)| *live).count()
    }
    /// Dead when no admissible (above-watermark, representable) opposite
    /// timestamp is still within the record's bounds.
    fn dead(&self, before: u64, after: u64, side: usize, record: Record) -> bool {
        let ahead = if side == 0 { after } else { before };
        let last = (i128::from(record.timestamp) + i128::from(ahead)).min(i128::from(u64::MAX));
        self.watermark[1 - side].is_some_and(|mark| i128::from(mark) >= last)
    }
}

const SIDES: [Side; 2] = [Side::Left, Side::Right];

#[allow(clippy::too_many_lines)]
fn run_model(seed: u64, before: u64, after: u64, base: u64) {
    const L: usize = 5;
    const R: usize = 7;
    let caps = [L, R];
    let mut draws = Draws(seed);
    let mut join = Join::<L, R>::new(Bounds::new(before, after));
    let mut model = Model::default();
    let mut clock = [base; 2];
    for _ in 0..4_000 {
        match draws.below(10) {
            0..=4 => {
                let side = usize::try_from(draws.below(2)).unwrap();
                let jitter = draws.below(12);
                let timestamp = (clock[side].saturating_sub(4)).saturating_add(jitter);
                let record = rec(draws.below(24), draws.below(3), timestamp);
                let expected = if !model.owed.is_empty() {
                    Err(Refusal::Pending)
                } else if model.watermark[side].is_some_and(|mark| timestamp <= mark) {
                    Err(Refusal::Late)
                } else if model.seen[side]
                    .iter()
                    .any(|(held, live)| *live && held.id == record.id)
                {
                    Err(Refusal::Duplicate)
                } else if model.live(side) == caps[side] {
                    Err(Refusal::Full)
                } else {
                    Ok(())
                };
                let snapshot = join.clone();
                assert_eq!(join.insert(SIDES[side], record), expected);
                if expected.is_err() {
                    assert_eq!(join, snapshot);
                    continue;
                }
                // Every record ever accepted on the other side, retired or not.
                for (other, _) in &model.seen[1 - side] {
                    let (left, right) = if side == 0 {
                        (record, *other)
                    } else {
                        (*other, record)
                    };
                    if Model::matches(before, after, left, right) {
                        model.owed.push(Pair {
                            key: record.key,
                            left: left.id,
                            right: right.id,
                        });
                    }
                }
                model.seen[side].push((record, true));
                assert_eq!(join.pending_pairs(), model.owed.len());
            }
            5 | 6 => {
                let mut buffer = [BLANK; 3];
                let room = usize::try_from(draws.below(4)).unwrap();
                let written = join.drain(&mut buffer[..room]);
                assert_eq!(written, room.min(model.owed.len()));
                for pair in &buffer[..written] {
                    let index = model.owed.iter().position(|owed| owed == pair);
                    model
                        .owed
                        .swap_remove(index.expect("emitted pair must be owed"));
                }
                assert_eq!(join.pending(), !model.owed.is_empty());
            }
            7 => {
                let side = usize::try_from(draws.below(2)).unwrap();
                let value = (clock[side].saturating_sub(2)).saturating_add(draws.below(6));
                let ok = model.watermark[side].is_none_or(|mark| value >= mark);
                let expected = if model.watermark[side] == Some(value) {
                    Ok(Advancing::Repeated)
                } else if ok {
                    Ok(Advancing::Advanced)
                } else {
                    Err(Refusal::WatermarkRegressed)
                };
                let snapshot = join.clone();
                assert_eq!(join.advance_watermark(SIDES[side], value), expected);
                if expected == Ok(Advancing::Repeated) {
                    assert_eq!(join, snapshot);
                }
                if ok {
                    model.watermark[side] = Some(value);
                    clock[side] = clock[side].max(value);
                } else {
                    assert_eq!(join, snapshot);
                }
                assert_eq!(join.watermark(SIDES[side]), model.watermark[side]);
            }
            _ => {
                let mut buffer = [GONE; 3];
                let room = usize::try_from(draws.below(4)).unwrap();
                let snapshot = join.clone();
                let result = join.retire(&mut buffer[..room]);
                if !model.owed.is_empty() {
                    assert_eq!(result, Err(Refusal::Pending));
                    assert_eq!(join, snapshot);
                    continue;
                }
                let due: usize = (0..2)
                    .map(|side| {
                        model.seen[side]
                            .iter()
                            .filter(|(held, live)| *live && model.dead(before, after, side, *held))
                            .count()
                    })
                    .sum();
                assert_eq!(snapshot.retirable(), due);
                assert_eq!(result, Ok(room.min(due)));
                for gone in &buffer[..room.min(due)] {
                    let side = usize::from(gone.side == Side::Right);
                    assert!(model.dead(before, after, side, gone.record));
                    let entry = model.seen[side]
                        .iter_mut()
                        .find(|(held, live)| *live && *held == gone.record)
                        .expect("retired record must be live");
                    entry.1 = false;
                }
            }
        }
        for (index, side) in SIDES.into_iter().enumerate() {
            assert_eq!(join.len(side), model.live(index));
        }
    }
}

#[test]
fn randomized_runs_match_the_naive_model() {
    for seed in 1..=40_u64 {
        let mut draws = Draws(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (before, after) = (draws.below(6), draws.below(6));
        run_model(seed, before, after, 0);
        run_model(seed + 1_000, before, after, u64::MAX - 40);
    }
    run_model(7, u64::MAX, u64::MAX, 0);
    run_model(8, 0, 0, 0);
}
