//! Thin binding to the bounded two-input interval join.
mod init;

use std::cell::RefCell;

use join_machines::{Bounds, Join, Pair, Record, Refusal, Retired, Side};
use magnus::scan_args::{get_kwargs, scan_args};
use magnus::{
    Error, Integer, RArray, RHash, RModule, Ruby, Symbol, TryConvert, Value, prelude::*,
    typed_data::Obj,
};

/// Live records held per input.
const CAPACITY: usize = 64;
/// Stack buffer for one core drain or retire call.
const CHUNK: usize = 16;

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

fn side(ruby: &Ruby, value: Value) -> Result<Side, Error> {
    let symbol = Symbol::from_value(value)
        .ok_or_else(|| Error::new(ruby.exception_type_error(), "side must be :left or :right"))?;
    match symbol.name()?.as_ref() {
        "left" => Ok(Side::Left),
        "right" => Ok(Side::Right),
        _ => Err(Error::new(
            ruby.exception_arg_error(),
            "side must be :left or :right",
        )),
    }
}

fn limit(max: Number) -> usize {
    usize::try_from(max.0).unwrap_or(usize::MAX)
}

fn data(ruby: &Ruby, name: &str) -> Result<Value, Error> {
    let module: RModule = ruby.class_object().const_get("JoinMachines")?;
    module.const_get(name)
}

#[magnus::wrap(class = "JoinMachines::Join", free_immediately, size)]
struct Wrapper {
    core: RefCell<Join<CAPACITY, CAPACITY>>,
}

impl Wrapper {
    fn new(args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (before, after) =
            get_kwargs::<_, (Number, Number), (), ()>(args.keywords, &["before", "after"], &[])?
                .required;
        Ok(Self {
            core: RefCell::new(Join::new(Bounds::new(before.0, after.0))),
        })
    }

    fn insert(
        ruby: &Ruby,
        this: Obj<Self>,
        input: Value,
        id: Number,
        key: Number,
        timestamp: Number,
    ) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let input = side(ruby, input)?;
        let record = Record {
            id: id.0,
            key: key.0,
            timestamp: timestamp.0,
        };
        let outcome = this.core.borrow_mut().insert(input, record);
        Ok(ruby.to_symbol(match outcome {
            Ok(()) => "accepted",
            Err(Refusal::Late) => "late",
            Err(Refusal::Duplicate) => "duplicate",
            Err(Refusal::Full) => "full",
            Err(_) => "pending",
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
        let class = data(ruby, "Pair")?;
        let mut remaining = limit(max);
        let result = ruby.ary_new();
        let blank = Pair {
            key: 0,
            left: 0,
            right: 0,
        };
        while remaining > 0 {
            let mut buffer = [blank; CHUNK];
            let room = remaining.min(CHUNK);
            let written = this.core.borrow_mut().drain(&mut buffer[..room]);
            for pair in &buffer[..written] {
                let value: Value = class.funcall("new", (pair.key, pair.left, pair.right))?;
                result.push(value)?;
            }
            remaining -= written;
            if written < room {
                break;
            }
        }
        Ok(result)
    }

    fn advance_watermark(
        ruby: &Ruby,
        this: Obj<Self>,
        input: Value,
        value: Number,
    ) -> Result<bool, Error> {
        this.check_frozen()?;
        let input = side(ruby, input)?;
        Ok(this.core.borrow_mut().advance_watermark(input, value.0))
    }

    fn watermark(ruby: &Ruby, this: &Self, input: Value) -> Result<Option<u64>, Error> {
        Ok(this.core.borrow().watermark(side(ruby, input)?))
    }

    fn retirable(&self) -> usize {
        self.core.borrow().retirable()
    }

    fn retire(ruby: &Ruby, this: Obj<Self>, max: Number) -> Result<Value, Error> {
        this.check_frozen()?;
        let class = data(ruby, "Retired")?;
        if this.core.borrow().pending() {
            return Ok(ruby.to_symbol("pending").as_value());
        }
        let mut remaining = limit(max);
        let result = ruby.ary_new();
        let blank = Retired {
            side: Side::Left,
            record: Record {
                id: 0,
                key: 0,
                timestamp: 0,
            },
        };
        while remaining > 0 {
            let mut buffer = [blank; CHUNK];
            let room = remaining.min(CHUNK);
            let Ok(written) = this.core.borrow_mut().retire(&mut buffer[..room]) else {
                break;
            };
            for gone in &buffer[..written] {
                let input = ruby.to_symbol(match gone.side {
                    Side::Left => "left",
                    Side::Right => "right",
                });
                let record = gone.record;
                let value: Value =
                    class.funcall("new", (input, record.id, record.key, record.timestamp))?;
                result.push(value)?;
            }
            remaining -= written;
            if written < room {
                break;
            }
        }
        Ok(result.as_value())
    }

    fn size(ruby: &Ruby, this: &Self, input: Value) -> Result<usize, Error> {
        Ok(this.core.borrow().len(side(ruby, input)?))
    }

    fn include(ruby: &Ruby, this: &Self, input: Value, id: Number) -> Result<bool, Error> {
        Ok(this.core.borrow().contains(side(ruby, input)?, id.0))
    }

    fn empty(&self) -> bool {
        self.core.borrow().is_empty()
    }

    const fn capacity() -> usize {
        CAPACITY
    }

    fn before(&self) -> u64 {
        self.core.borrow().bounds().before()
    }

    fn after(&self) -> u64 {
        self.core.borrow().bounds().after()
    }
}
