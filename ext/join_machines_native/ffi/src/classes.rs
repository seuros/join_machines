//! The gem's `Data` classes, resolved once when the extension loads.
//!
//! Sealing in Ruby stops accidents, not reflection: a subclass constant is
//! bound before `inherited` raises, `Data#initialize` can be bound directly
//! and `Marshal.load` skips `initialize`. So the binding is the authority: a
//! value it reads must be an instance of exactly the sealed class, its members
//! are read natively (no method is dispatched) and the caller re-validates
//! them through the core.
use magnus::{Error, RModule, RStruct, Ruby, Value, prelude::*};
use matryona_machines::ruby::{Class, Registry, exact_fields, instance_of};

/// `JoinMachines::Reach`, `Bounds`, `Pair`, `Record`, `Retired`, `Watermark`.
pub struct Classes {
    pub reach: Class,
    pub bounds: Class,
    pub pair: Class,
    pub record: Class,
    pub retired: Class,
    pub watermark: Class,
}

static CLASSES: Registry<Classes> = Registry::new("join_machines");

/// Resolve and pin the classes Ruby defined before loading the extension.
pub fn resolve(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |name| Class::resolve(ruby, module, name);
    let classes = Classes {
        reach: class("Reach")?,
        bounds: class("Bounds")?,
        pair: class("Pair")?,
        record: class("Record")?,
        retired: class("Retired")?,
        watermark: class("Watermark")?,
    };
    CLASSES.set(ruby, classes).map(drop)
}

/// The classes resolved at init.
pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get(ruby)
}

/// A new value of `class`, through its own `new` (so through its check).
pub fn build<A: magnus::ArgList>(ruby: &Ruby, class: Class, args: A) -> Result<Value, Error> {
    class.get(ruby).funcall("new", args)
}

/// The `N` members of `value`, read from the struct (no reader runs), when
/// `value` is an instance of exactly `class` (`instance_of?`, not `is_a?`);
/// `TypeError` with `message` otherwise.
pub fn members<const N: usize>(
    ruby: &Ruby,
    value: Value,
    class: Class,
    message: &'static str,
) -> Result<[Value; N], Error> {
    exact_fields::<N>(ruby, value, class.get(ruby), message)
}

/// Whether `value` is an instance of a subclass of `class` (never `class`
/// itself): a value that claims to be one of the gem's and is not.
pub fn subclassed(ruby: &Ruby, value: Value, class: Class) -> bool {
    let class = class.get(ruby);
    value.is_kind_of(class)
        && !RStruct::from_value(value).is_some_and(|data| instance_of(data, class))
}
