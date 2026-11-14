//! The gem's `Data` classes, resolved once when the extension loads.
//!
//! Sealing in Ruby stops accidents, not reflection: a subclass constant is
//! bound before `inherited` raises, `Data#initialize` can be bound directly
//! and `Marshal.load` skips `initialize`. So the binding is the authority: a
//! value it reads must be an instance of exactly the sealed class, its members
//! are read natively (no method is dispatched) and the caller re-validates
//! them through the core.
use std::sync::OnceLock;

use magnus::{
    Error, RClass, RModule, RStruct, Ruby, Value, prelude::*, rb_sys::AsRawValue, value::Opaque,
};

/// `JoinMachines::Reach`, `Bounds`, `Pair`, `Record`, `Retired`, `Watermark`.
pub struct Classes {
    pub reach: Opaque<RClass>,
    pub bounds: Opaque<RClass>,
    pub pair: Opaque<RClass>,
    pub record: Opaque<RClass>,
    pub retired: Opaque<RClass>,
    pub watermark: Opaque<RClass>,
}

static CLASSES: OnceLock<Classes> = OnceLock::new();

/// Resolve and pin the classes Ruby defined before loading the extension.
pub fn resolve(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |name: &str| -> Result<Opaque<RClass>, Error> {
        let class: RClass = module.const_get(name)?;
        // Pinned: compaction must not move what the static points at.
        ruby.gc_register_mark_object(class);
        Ok(class.into())
    };
    let classes = Classes {
        reach: class("Reach")?,
        bounds: class("Bounds")?,
        pair: class("Pair")?,
        record: class("Record")?,
        retired: class("Retired")?,
        watermark: class("Watermark")?,
    };
    CLASSES.set(classes).map_err(|_| {
        Error::new(
            ruby.exception_runtime_error(),
            "the join_machines extension was initialised twice",
        )
    })
}

/// The classes resolved at init.
pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get().ok_or_else(|| {
        Error::new(
            ruby.exception_runtime_error(),
            "the join_machines extension is not initialised",
        )
    })
}

/// A new value of `class`, through its own `new` (so through its check).
pub fn build<A: magnus::ArgList>(
    ruby: &Ruby,
    class: Opaque<RClass>,
    args: A,
) -> Result<Value, Error> {
    ruby.get_inner(class).funcall("new", args)
}

/// The `N` members of `value`, read natively, when `value` is an instance of
/// exactly `class` (`instance_of?`, not `is_a?`); `TypeError` otherwise.
pub fn members<const N: usize>(
    ruby: &Ruby,
    value: Value,
    class: Opaque<RClass>,
) -> Result<[Value; N], Error> {
    let class = ruby.get_inner(class);
    let data = RStruct::from_value(value)
        .filter(|data| instance_of(*data, class))
        .ok_or_else(|| Error::new(ruby.exception_type_error(), format!("expected a {class}")))?;
    let mut out = [ruby.qnil().as_value(); N];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = data.aref(index)?;
    }
    Ok(out)
}

/// Whether `value` is an instance of a subclass of `class` (never `class`
/// itself): a value that claims to be one of the gem's and is not.
pub fn subclassed(ruby: &Ruby, value: Value, class: Opaque<RClass>) -> bool {
    let class = ruby.get_inner(class);
    value.is_kind_of(class)
        && !RStruct::from_value(value).is_some_and(|data| instance_of(data, class))
}

/// Pointer equality on the classes, so no `==` the caller could redefine runs.
/// A singleton class on the instance itself does not change what it is.
fn instance_of(value: RStruct, class: RClass) -> bool {
    let actual = value.class();
    if actual.as_raw() == class.as_raw() {
        return true;
    }
    let singleton = value
        .singleton_class()
        .is_ok_and(|own| own.as_raw() == actual.as_raw());
    singleton
        && actual
            .superclass()
            .is_ok_and(|parent| parent.as_raw() == class.as_raw())
}
