//! Thin binding to the bounded two-input interval join.
mod init;

use std::cell::RefCell;

use join_machines::{Advancing, Bounds, Join, Pair, Record, Refusal, Retired, Side, Watermark};
use magnus::{
    Error, Integer, RArray, RClass, RModule, Ruby, Symbol, TryConvert, Value, prelude::*,
    typed_data::Obj,
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

fn data(ruby: &Ruby, name: &str) -> Result<Value, Error> {
    let module: RModule = ruby.class_object().const_get("JoinMachines")?;
    module.const_get(name)
}

/// A `JoinMachines::<name>` value, or `TypeError` for anything else.
fn value_of(value: Value, name: &str) -> Result<Value, Error> {
    let ruby = Ruby::get_with(value);
    let class = RClass::try_convert(data(&ruby, name)?)?;
    if value.is_kind_of(class) {
        Ok(value)
    } else {
        Err(Error::new(
            ruby.exception_type_error(),
            format!("expected a JoinMachines::{name}"),
        ))
    }
}

fn field(value: Value, name: &str) -> Result<u64, Error> {
    let raw: Value = value.funcall(name, ())?;
    Number::try_convert(raw).map(|number| number.0)
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
                ruby.exception_arg_error(),
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

/// A `JoinMachines::Bounds`, read field by field so both are validated.
#[derive(Clone, Copy)]
struct Limits(Bounds);

impl TryConvert for Limits {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let value = value_of(value, "Bounds")?;
        Ok(Self(Bounds::new(
            field(value, "before")?,
            field(value, "after")?,
        )))
    }
}

/// A `JoinMachines::Record`, read field by field so every one is validated.
#[derive(Clone, Copy)]
struct Entry(Record);

impl TryConvert for Entry {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let value = value_of(value, "Record")?;
        Ok(Self(Record {
            id: field(value, "id")?,
            key: field(value, "key")?,
            timestamp: field(value, "timestamp")?,
        }))
    }
}

/// Ruby symbol name of a refusal: the snake case of the core variant.
const fn refusal(refused: Refusal) -> &'static str {
    match refused {
        Refusal::Pending => "pending",
        Refusal::Late => "late",
        Refusal::Duplicate => "duplicate",
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

/// `Bounds#reach(side, timestamp)`: the `[first, last]` of the core's `Reach`.
fn bounds_reach(this: Value, input: Input, timestamp: Number) -> Result<(u64, u64), Error> {
    let reach = Limits::try_convert(this)?.0.reach(input.0, timestamp.0);
    Ok((reach.first, reach.last))
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
        data(ruby, "Bounds")?.funcall("new", (bounds.before(), bounds.after()))
    }

    fn insert(ruby: &Ruby, this: Obj<Self>, input: Input, record: Entry) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().insert(input.0, record.0);
        Ok(ruby.to_symbol(match outcome {
            Ok(()) => "accepted",
            Err(refused) => refusal(refused),
        }))
    }

    fn pending(&self) -> bool {
        self.core.borrow().pending()
    }

    fn pending_pairs(&self) -> usize {
        self.core.borrow().pending_pairs()
    }

    fn drain(ruby: &Ruby, this: Obj<Self>, max: Number) -> Result<RArray, Error> {
        this.check_frozen()?;
        // Resolve the class before taking anything, so a failure loses nothing.
        let class = data(ruby, "Pair")?;
        let pairs = {
            let mut core = this.core.borrow_mut();
            let room = usize::try_from(max.0).unwrap_or(usize::MAX);
            let mut buffer = vec![Pair::default(); room.min(core.pending_pairs())];
            let written = core.drain(&mut buffer);
            buffer.truncate(written);
            buffer
        };
        let result = ruby.ary_new_capa(pairs.len());
        for pair in pairs {
            let value: Value = class.funcall("new", (pair.key, pair.left, pair.right))?;
            result.push(value)?;
        }
        Ok(result)
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

    /// Reads `watermark.value` (an Integer, or nil for "nothing observed yet")
    /// into the sibling type the core takes.
    fn advance_watermark_from(
        ruby: &Ruby,
        this: Obj<Self>,
        input: Input,
        watermark: Value,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        if !watermark.respond_to("value", false)? {
            return Err(Error::new(
                ruby.exception_type_error(),
                "watermark must respond to #value",
            ));
        }
        let value: Option<Value> = watermark.funcall("value", ())?;
        let mut source = Watermark::default();
        if let Some(value) = value {
            // A fresh watermark takes any first value.
            let _first = source.advance(Number::try_convert(value)?.0);
        }
        let outcome = this
            .core
            .borrow_mut()
            .advance_watermark_from(input.0, &source);
        Ok(advancing(ruby, outcome))
    }

    fn watermark(&self, input: Input) -> Option<u64> {
        self.core.borrow().watermark(input.0)
    }

    fn side_watermark(ruby: &Ruby, this: &Self, input: Input) -> Result<Value, Error> {
        let watermark = this.core.borrow().side_watermark(input.0);
        data(ruby, "Watermark")?.funcall("new", (watermark.value(),))
    }

    fn retirable(&self) -> usize {
        self.core.borrow().retirable()
    }

    fn retire(ruby: &Ruby, this: Obj<Self>, max: Number) -> Result<Value, Error> {
        this.check_frozen()?;
        // Resolve the classes before removing anything, so a failure loses nothing.
        let (class, record_class) = (data(ruby, "Retired")?, data(ruby, "Record")?);
        let retired = {
            let mut core = this.core.borrow_mut();
            let room = usize::try_from(max.0).unwrap_or(usize::MAX);
            let mut buffer = vec![Retired::default(); room.min(core.retirable())];
            match core.retire(&mut buffer) {
                Ok(written) => buffer.truncate(written),
                Err(refused) => return Ok(ruby.to_symbol(refusal(refused)).as_value()),
            }
            buffer
        };
        let result = ruby.ary_new_capa(retired.len());
        for gone in retired {
            let held = gone.record;
            let record: Value = record_class.funcall("new", (held.id, held.key, held.timestamp))?;
            let input = ruby.to_symbol(side_name(gone.side));
            let value: Value = class.funcall("new", (input, record))?;
            result.push(value)?;
        }
        Ok(result.as_value())
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
