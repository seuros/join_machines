//! Thin binding to the bounded two-input interval join.
mod classes;
mod init;

use std::cell::RefCell;

use classes::{build, classes, members};
use join_machines::{
    Advancing, Bounds, Inserting, Join, Pair, Reach, Record, Refusal, Retired, Side, Watermark,
};
use magnus::{
    Error, Integer, RArray, Ruby, Symbol, TryConvert, Value, typed_data::Obj, value::ReprValue,
};

/// Live records held per input.
const CAPACITY: usize = 64;

#[derive(Clone, Copy)]
struct Number(u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
                "expected an Integer (unsigned 64-bit)",
            ));
        }
        u64::try_convert(value).map(Self)
    }
}

/// Runs `act` on a copy of the cell's value and keeps the copy only once the
/// answer `act` builds exists, so a raise while building it (a failed
/// allocation, a redefined `new`) loses nothing. No `RefCell` guard is alive
/// while Ruby runs. A build that froze `owner` meanwhile raises
/// `FrozenError`, one that changed the value (a redefined `new` calling back
/// in) `RuntimeError`; neither applies the copy.
fn on_copy<T: Clone + PartialEq, R>(
    ruby: &Ruby,
    owner: impl ReprValue,
    cell: &RefCell<T>,
    act: impl FnOnce(&mut T) -> Result<R, Error>,
) -> Result<R, Error> {
    owner.check_frozen()?;
    let before = cell.borrow().clone();
    let mut after = before.clone();
    let answer = act(&mut after)?;
    owner.check_frozen()?;
    let mut held = cell.borrow_mut();
    if *held != before {
        return Err(Error::new(
            ruby.exception_runtime_error(),
            "the join changed while the answer was built; the call was not applied",
        ));
    }
    *held = after;
    Ok(answer)
}

/// A Rust buffer for at most `max` entries, of which `ready` exist.
fn buffer<T: Clone + Default>(max: Number, ready: usize) -> Vec<T> {
    let room = usize::try_from(max.0).unwrap_or(usize::MAX);
    vec![T::default(); room.min(ready)]
}

/// `:left` or `:right`.
#[derive(Clone, Copy)]
struct Input(Side);

impl TryConvert for Input {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let ruby = Ruby::get_with(value);
        let symbol = Symbol::from_value(value).ok_or_else(|| {
            Error::new(ruby.exception_type_error(), "side must be :left or :right")
        })?;
        match symbol.name()?.as_ref() {
            "left" => Ok(Self(Side::Left)),
            "right" => Ok(Self(Side::Right)),
            _ => Err(Error::new(
                ruby.exception_range_error(),
                "side must be :left or :right",
            )),
        }
    }
}

const fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// The members of a gem value (exactly its class, read natively), each
/// converted to a `Number`: a forged value raises, it never reaches the core.
fn numbers<const N: usize>(
    value: Value,
    class: impl FnOnce(&classes::Classes) -> magnus::value::Opaque<magnus::RClass>,
) -> Result<[Number; N], Error> {
    let ruby = Ruby::get_with(value);
    let read = members::<N>(&ruby, value, class(classes(&ruby)?))?;
    let mut out = [Number(0); N];
    for (slot, member) in out.iter_mut().zip(read) {
        *slot = Number::try_convert(member)?;
    }
    Ok(out)
}

/// A `JoinMachines::Bounds`, re-validated through the core's `Bounds::new`.
#[derive(Clone, Copy)]
struct Limits(Bounds);

impl TryConvert for Limits {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let [before, after] = numbers(value, |known| known.bounds)?;
        Ok(Self(Bounds::new(before.0, after.0)))
    }
}

/// A `JoinMachines::Record`: every field an Integer in `0..2**64`.
#[derive(Clone, Copy)]
struct Entry(Record);

impl TryConvert for Entry {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let [id, key, timestamp] = numbers(value, |known| known.record)?;
        Ok(Self(Record {
            id: id.0,
            key: key.0,
            timestamp: timestamp.0,
        }))
    }
}

/// The core's `Reach::new`: what `Reach.new` is held to.
fn checked_reach(ruby: &Ruby, first: Number, last: Number) -> Result<Reach, Error> {
    Reach::new(first.0, last.0).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "a reach's first must not be above its last",
        )
    })
}

/// Private hook behind `Reach.new`.
fn validate_reach(ruby: &Ruby, first: Number, last: Number) -> Result<(), Error> {
    checked_reach(ruby, first, last).map(|_| ())
}

/// Private hooks behind `Bounds.new`, `Pair.new`, `Record.new`, `Retired.new`
/// and `Watermark.new`. The core's `Bounds::new` and its public-field `Pair`,
/// `Record` and `Retired` accept every value of their field types, so
/// converting each argument to that type (an Integer in `0..2**64`, a side, a
/// `JoinMachines::Record`, `nil` or an Integer) is the whole check.
const fn validate_bounds(_before: Number, _after: Number) {}
const fn validate_pair(_key: Number, _left: Number, _right: Number) {}
const fn validate_record(_id: Number, _key: Number, _timestamp: Number) {}
const fn validate_retired(_input: Input, _record: Entry) {}
const fn validate_watermark(_value: Option<Number>) {}

/// `nil` or an Integer in `0..2**64`, as the sibling's `Watermark`.
fn watermark_of(value: Option<Number>) -> Watermark {
    value.map_or_else(Watermark::default, |value| Watermark::at(value.0))
}

/// `Reach#include?`: the core's `Reach::contains`.
fn reach_contains(ruby: &Ruby, this: Value, timestamp: Number) -> Result<bool, Error> {
    let [first, last] = numbers(this, |known| known.reach)?;
    Ok(checked_reach(ruby, first, last)?.contains(timestamp.0))
}

/// Ruby symbol name of a refusal: the snake case of the core variant.
const fn refusal(refused: Refusal) -> &'static str {
    match refused {
        Refusal::Mismatch => "mismatch",
        Refusal::Pending => "pending",
        Refusal::Late => "late",
        Refusal::Full => "full",
        Refusal::WatermarkRegressed => "watermark_regressed",
    }
}

fn advancing(ruby: &Ruby, outcome: Result<Advancing, Refusal>) -> Symbol {
    ruby.to_symbol(match outcome {
        Ok(Advancing::Advanced) => "advanced",
        Ok(Advancing::Repeated) => "repeated",
        Err(refused) => refusal(refused),
    })
}

/// `Bounds#reach(side, timestamp)`: the core's `Reach`, as a `Reach`.
fn bounds_reach(ruby: &Ruby, this: Value, input: Input, timestamp: Number) -> Result<Value, Error> {
    let reach = Limits::try_convert(this)?.0.reach(input.0, timestamp.0);
    build(ruby, classes(ruby)?.reach, (reach.first(), reach.last()))
}

/// `Side::other` for the Symbol that stands for a side.
fn other(ruby: &Ruby, input: Input) -> Symbol {
    ruby.to_symbol(side_name(input.0.other()))
}

#[magnus::wrap(class = "JoinMachines::Join", free_immediately, size)]
struct Wrapper {
    core: RefCell<Join<CAPACITY, CAPACITY>>,
}

impl Wrapper {
    fn new(bounds: Limits) -> Self {
        Self {
            core: RefCell::new(Join::new(bounds.0)),
        }
    }

    fn bounds(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let bounds = this.core.borrow().bounds();
        build(
            ruby,
            classes(ruby)?.bounds,
            (bounds.before(), bounds.after()),
        )
    }

    fn insert(ruby: &Ruby, this: Obj<Self>, input: Input, record: Entry) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().insert(input.0, record.0);
        Ok(ruby.to_symbol(match outcome {
            Ok(Inserting::Accepted) => "accepted",
            Ok(Inserting::Duplicate) => "duplicate",
            Err(refused) => refusal(refused),
        }))
    }

    fn pending(&self) -> bool {
        self.core.borrow().pending()
    }

    fn pending_pairs(&self) -> usize {
        self.core.borrow().pending_pairs()
    }

    /// Drains into a Rust buffer and builds every `Pair` through [`on_copy`]:
    /// a raise while building leaves every pair owed, a freeze meanwhile
    /// raises `FrozenError`, a re-entrant change `RuntimeError`.
    fn drain(ruby: &Ruby, this: Obj<Self>, max: Number) -> Result<RArray, Error> {
        let class = classes(ruby)?.pair;
        on_copy(ruby, this, &this.core, |next| {
            let mut buffer = buffer::<Pair>(max, next.pending_pairs());
            let written = next.drain(&mut buffer);
            let result = ruby.ary_new_capa(written);
            for pair in &buffer[..written] {
                result.push(build(ruby, class, (pair.key, pair.left, pair.right))?)?;
            }
            Ok(result)
        })
    }

    fn advance_watermark(
        ruby: &Ruby,
        this: Obj<Self>,
        input: Input,
        value: Number,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().advance_watermark(input.0, value.0);
        Ok(advancing(ruby, outcome))
    }

    /// Reads the watermark's value (an Integer, or nil for "nothing observed
    /// yet") into the sibling type the core takes: natively from this gem's
    /// own `Watermark`, through `#value` from a sibling's. A `#value` that
    /// froze the join raises `FrozenError` before the core is called.
    fn advance_watermark_from(
        ruby: &Ruby,
        this: Obj<Self>,
        input: Input,
        watermark: Value,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let source = watermark_of(Self::watermark_value(ruby, watermark)?);
        this.check_frozen()?;
        let outcome = this
            .core
            .borrow_mut()
            .advance_watermark_from(input.0, &source);
        Ok(advancing(ruby, outcome))
    }

    /// `watermark`'s value: this gem's `Watermark` exactly (read natively),
    /// `TypeError` for a subclass of it, else a sibling's read by `#value`.
    fn watermark_value(ruby: &Ruby, watermark: Value) -> Result<Option<Number>, Error> {
        let own = classes(ruby)?.watermark;
        if let Ok([value]) = members::<1>(ruby, watermark, own) {
            return TryConvert::try_convert(value);
        }
        if classes::subclassed(ruby, watermark, own) {
            return Err(Error::new(
                ruby.exception_type_error(),
                "JoinMachines::Watermark is sealed: a subclass of it is not a Watermark",
            ));
        }
        if !watermark.respond_to("value", false)? {
            return Err(Error::new(
                ruby.exception_type_error(),
                "watermark must respond to #value",
            ));
        }
        watermark.funcall("value", ())
    }

    fn watermark(&self, input: Input) -> Option<u64> {
        self.core.borrow().watermark(input.0)
    }

    fn side_watermark(ruby: &Ruby, this: &Self, input: Input) -> Result<Value, Error> {
        let watermark = this.core.borrow().side_watermark(input.0);
        build(ruby, classes(ruby)?.watermark, (watermark.value(),))
    }

    fn retirable(&self) -> usize {
        self.core.borrow().retirable()
    }

    /// Retires into a Rust buffer and builds every `Retired` through
    /// [`on_copy`]: a raise while building leaves every record held, a
    /// freeze meanwhile raises `FrozenError`, a re-entrant change
    /// `RuntimeError`.
    fn retire(ruby: &Ruby, this: Obj<Self>, max: Number) -> Result<Value, Error> {
        let known = classes(ruby)?;
        on_copy(ruby, this, &this.core, |next| {
            let mut buffer = buffer::<Retired>(max, next.retirable());
            let written = match next.retire(&mut buffer) {
                Ok(written) => written,
                Err(refused) => return Ok(ruby.to_symbol(refusal(refused)).as_value()),
            };
            let result = ruby.ary_new_capa(written);
            for gone in &buffer[..written] {
                let held = gone.record;
                let record = build(ruby, known.record, (held.id, held.key, held.timestamp))?;
                let input = ruby.to_symbol(side_name(gone.side));
                result.push(build(ruby, known.retired, (input, record))?)?;
            }
            Ok(result.as_value())
        })
    }

    fn size(&self, input: Input) -> usize {
        self.core.borrow().len(input.0)
    }

    fn include(&self, input: Input, id: Number) -> bool {
        self.core.borrow().contains(input.0, id.0)
    }

    fn empty(&self) -> bool {
        self.core.borrow().is_empty()
    }

    fn capacity(&self, input: Input) -> usize {
        self.core.borrow().capacity(input.0)
    }
}
