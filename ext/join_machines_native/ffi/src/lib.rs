//! Thin binding to the bounded two-input interval join.
mod classes;
mod init;

use std::cell::RefCell;

use classes::{build, classes, members};
use join_machines::{
    Advancing, Bounds, Inserting, Join, Pair, Reach, Record, Refusal, Retired, Side, Watermark,
};
use magnus::{Error, RArray, Ruby, Symbol, Value, typed_data::Obj, value::ReprValue};
use matryona_machines::ruby::{Class, Symbols, integer, keep, optional, own_or_sibling, symbol};

/// Live records held per input.
const CAPACITY: usize = 64;

/// A Rust buffer for at most `max` entries, of which `ready` exist.
fn buffer<T: Clone + Default>(max: u64, ready: usize) -> Vec<T> {
    let room = usize::try_from(max).unwrap_or(usize::MAX);
    vec![T::default(); room.min(ready)]
}

matryona_machines::symbols! {
    /// The sibling's `Side` as `:left` and `:right`.
    Sides for Side as "side" {
        Left => "left",
        Right => "right",
    }
}

/// `:left` or `:right`: `TypeError` for a non-Symbol, `RangeError` for
/// another Symbol.
fn side(value: Value) -> Result<Side, Error> {
    Sides::parse(&Ruby::get_with(value), value)
}

/// The members of a gem value (exactly its class, read natively), each
/// converted to a `u64` under its member's name: a forged value raises, it
/// never reaches the core.
fn numbers<const N: usize>(
    value: Value,
    class: impl FnOnce(&classes::Classes) -> Class,
    message: &'static str,
    names: [&str; N],
) -> Result<[u64; N], Error> {
    let ruby = Ruby::get_with(value);
    let read = members::<N>(&ruby, value, class(classes(&ruby)?), message)?;
    let mut out = [0; N];
    for ((slot, member), name) in out.iter_mut().zip(read).zip(names) {
        *slot = integer(member, name)?;
    }
    Ok(out)
}

/// A `JoinMachines::Bounds`, re-validated through the core's `Bounds::new`.
fn limits(value: Value) -> Result<Bounds, Error> {
    let [before, after] = numbers(
        value,
        |known| known.bounds,
        "expected a JoinMachines::Bounds",
        ["before", "after"],
    )?;
    Ok(Bounds::new(before, after))
}

/// A `JoinMachines::Record`: every field an Integer in `0..2**64`.
fn entry(value: Value) -> Result<Record, Error> {
    let names = ["id", "key", "timestamp"];
    let [id, key, timestamp] = numbers(
        value,
        |known| known.record,
        "expected a JoinMachines::Record",
        names,
    )?;
    Ok(Record { id, key, timestamp })
}

/// The core's `Reach::new`: what `Reach.new` is held to.
fn checked_reach(ruby: &Ruby, first: u64, last: u64) -> Result<Reach, Error> {
    Reach::new(first, last).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "a reach's first must not be above its last",
        )
    })
}

/// Private hook behind `Reach.new`.
fn validate_reach(ruby: &Ruby, first: Value, last: Value) -> Result<(), Error> {
    let (first, last) = (integer(first, "first")?, integer(last, "last")?);
    checked_reach(ruby, first, last).map(|_| ())
}

/// Private hooks behind `Bounds.new`, `Pair.new`, `Record.new`, `Retired.new`
/// and `Watermark.new`. The core's `Bounds::new` and its public-field `Pair`,
/// `Record` and `Retired` accept every value of their field types, so
/// converting each argument to that type (an Integer in `0..2**64`, a side, a
/// `JoinMachines::Record`, `nil` or an Integer) is the whole check.
fn validate_bounds(before: Value, after: Value) -> Result<(), Error> {
    integer::<u64>(before, "before")?;
    integer::<u64>(after, "after").map(drop)
}

fn validate_pair(key: Value, left: Value, right: Value) -> Result<(), Error> {
    integer::<u64>(key, "key")?;
    integer::<u64>(left, "left")?;
    integer::<u64>(right, "right").map(drop)
}

fn validate_record(id: Value, key: Value, timestamp: Value) -> Result<(), Error> {
    integer::<u64>(id, "id")?;
    integer::<u64>(key, "key")?;
    integer::<u64>(timestamp, "timestamp").map(drop)
}

fn validate_retired(input: Value, record: Value) -> Result<(), Error> {
    side(input)?;
    entry(record).map(drop)
}

fn validate_watermark(value: Value) -> Result<(), Error> {
    optional::<u64>(value, "value").map(drop)
}

/// `nil` or an Integer in `0..2**64`, as the sibling's `Watermark`.
fn watermark_of(value: Option<u64>) -> Watermark {
    value.map_or_else(Watermark::default, Watermark::at)
}

/// `Reach#include?`: the core's `Reach::contains`.
fn reach_contains(ruby: &Ruby, this: Value, timestamp: Value) -> Result<bool, Error> {
    let timestamp = integer(timestamp, "timestamp")?;
    let [first, last] = numbers(
        this,
        |known| known.reach,
        "expected a JoinMachines::Reach",
        ["first", "last"],
    )?;
    Ok(checked_reach(ruby, first, last)?.contains(timestamp))
}

matryona_machines::symbols! {
    /// The core's refusals, each the snake case of its variant.
    Refusals for Refusal as "a refusal" {
        Mismatch => "mismatch",
        Pending => "pending",
        Late => "late",
        Full => "full",
        WatermarkRegressed => "watermark_regressed",
    }
}

matryona_machines::symbols! {
    /// An honoured watermark advance.
    Advancings for Advancing as "an advance" {
        Advanced => "advanced",
        Repeated => "repeated",
    }
}

matryona_machines::symbols! {
    /// An honoured insert.
    Insertings for Inserting as "an insert" {
        Accepted => "accepted",
        Duplicate => "duplicate",
    }
}

fn advancing(ruby: &Ruby, outcome: Result<Advancing, Refusal>) -> Symbol {
    symbol(ruby, outcome.map_or_else(Refusals::name, Advancings::name))
}

/// `Bounds#reach(side, timestamp)`: the core's `Reach`, as a `Reach`.
fn bounds_reach(ruby: &Ruby, this: Value, input: Value, timestamp: Value) -> Result<Value, Error> {
    let (input, timestamp) = (side(input)?, integer(timestamp, "timestamp")?);
    let reach = limits(this)?.reach(input, timestamp);
    build(ruby, classes(ruby)?.reach, (reach.first(), reach.last()))
}

/// `Side::other` for the Symbol that stands for a side.
fn other(ruby: &Ruby, input: Value) -> Result<Symbol, Error> {
    Ok(Sides::symbol(ruby, side(input)?.other()))
}

#[magnus::wrap(class = "JoinMachines::Join", free_immediately, size)]
struct Wrapper {
    core: RefCell<Join<CAPACITY, CAPACITY>>,
}

impl Wrapper {
    fn new(bounds: Value) -> Result<Self, Error> {
        Ok(Self {
            core: RefCell::new(Join::new(limits(bounds)?)),
        })
    }

    fn bounds(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let bounds = this.core.borrow().bounds();
        build(
            ruby,
            classes(ruby)?.bounds,
            (bounds.before(), bounds.after()),
        )
    }

    fn insert(ruby: &Ruby, this: Obj<Self>, input: Value, record: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let (input, record) = (side(input)?, entry(record)?);
        let outcome = this.core.borrow_mut().insert(input, record);
        Ok(symbol(
            ruby,
            outcome.map_or_else(Refusals::name, Insertings::name),
        ))
    }

    fn pending(&self) -> bool {
        self.core.borrow().pending()
    }

    fn pending_pairs(&self) -> usize {
        self.core.borrow().pending_pairs()
    }

    /// Drains into a Rust buffer and builds every `Pair` through [`keep`]: a
    /// raise while building leaves every pair owed, a freeze meanwhile
    /// raises `FrozenError`, a re-entrant change `RuntimeError`.
    fn drain(ruby: &Ruby, this: Obj<Self>, max: Value) -> Result<RArray, Error> {
        this.check_frozen()?;
        let max = integer(max, "max")?;
        let class = classes(ruby)?.pair;
        keep(
            ruby,
            this,
            &this.core,
            |next| {
                let mut buffer = buffer::<Pair>(max, next.pending_pairs());
                let written = next.drain(&mut buffer);
                buffer.truncate(written);
                buffer
            },
            |drained| {
                let result = ruby.ary_new_capa(drained.len());
                for pair in drained {
                    result.push(build(ruby, class, (pair.key, pair.left, pair.right))?)?;
                }
                Ok(result)
            },
        )
    }

    fn advance_watermark(
        ruby: &Ruby,
        this: Obj<Self>,
        input: Value,
        value: Value,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let (input, value) = (side(input)?, integer(value, "value")?);
        let outcome = this.core.borrow_mut().advance_watermark(input, value);
        Ok(advancing(ruby, outcome))
    }

    /// Reads the watermark's value (an Integer, or nil for "nothing observed
    /// yet") into the sibling type the core takes: natively from this gem's
    /// own `Watermark`, through `#value` from a sibling's. A `#value` that
    /// froze the join raises `FrozenError` before the core is called.
    fn advance_watermark_from(
        ruby: &Ruby,
        this: Obj<Self>,
        input: Value,
        watermark: Value,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let input = side(input)?;
        let source = watermark_of(Self::watermark_value(ruby, watermark)?);
        this.check_frozen()?;
        let outcome = this
            .core
            .borrow_mut()
            .advance_watermark_from(input, &source);
        Ok(advancing(ruby, outcome))
    }

    /// `watermark`'s value: this gem's `Watermark` exactly (read natively),
    /// `TypeError` for a subclass of it, else a sibling's read by `#value`.
    fn watermark_value(ruby: &Ruby, watermark: Value) -> Result<Option<u64>, Error> {
        let own = classes(ruby)?.watermark.get(ruby);
        let [value] = own_or_sibling(
            ruby,
            watermark,
            own,
            ["value"],
            "watermark must be a JoinMachines::Watermark or respond to #value",
        )?;
        optional(value, "value")
    }

    fn watermark(&self, input: Value) -> Result<Option<u64>, Error> {
        Ok(self.core.borrow().watermark(side(input)?))
    }

    fn side_watermark(ruby: &Ruby, this: &Self, input: Value) -> Result<Value, Error> {
        let watermark = this.core.borrow().side_watermark(side(input)?);
        build(ruby, classes(ruby)?.watermark, (watermark.value(),))
    }

    fn retirable(&self) -> usize {
        self.core.borrow().retirable()
    }

    /// Retires into a Rust buffer and builds every `Retired` through
    /// [`keep`]: a raise while building leaves every record held, a freeze
    /// meanwhile raises `FrozenError`, a re-entrant change `RuntimeError`.
    fn retire(ruby: &Ruby, this: Obj<Self>, max: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let max = integer(max, "max")?;
        let known = classes(ruby)?;
        keep(
            ruby,
            this,
            &this.core,
            |next| {
                let mut buffer = buffer::<Retired>(max, next.retirable());
                next.retire(&mut buffer).map(|written| {
                    buffer.truncate(written);
                    buffer
                })
            },
            |retired| {
                let retired = match retired {
                    Ok(retired) => retired,
                    Err(refused) => return Ok(Refusals::symbol(ruby, refused).as_value()),
                };
                let result = ruby.ary_new_capa(retired.len());
                for gone in retired {
                    let held = gone.record;
                    let record = build(ruby, known.record, (held.id, held.key, held.timestamp))?;
                    let input = Sides::symbol(ruby, gone.side);
                    result.push(build(ruby, known.retired, (input, record))?)?;
                }
                Ok(result.as_value())
            },
        )
    }

    fn size(&self, input: Value) -> Result<usize, Error> {
        Ok(self.core.borrow().len(side(input)?))
    }

    fn include(&self, input: Value, id: Value) -> Result<bool, Error> {
        let (input, id) = (side(input)?, integer(id, "id")?);
        Ok(self.core.borrow().contains(input, id))
    }

    fn empty(&self) -> bool {
        self.core.borrow().is_empty()
    }

    fn capacity(&self, input: Value) -> Result<usize, Error> {
        Ok(self.core.borrow().capacity(side(input)?))
    }
}
