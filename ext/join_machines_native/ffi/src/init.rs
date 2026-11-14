//! Private native entry point.
use crate::{
    CAPACITY, Wrapper, bounds_reach, other, reach_contains, validate_bounds, validate_pair,
    validate_reach, validate_record, validate_retired, validate_watermark,
};
use magnus::{Error, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("JoinMachines")?;
    module.const_set("CAPACITY", CAPACITY)?;
    crate::classes::resolve(ruby, module)?;
    module.define_singleton_method("other", function!(other, 1))?;
    // Bounds is a Data class defined in Ruby; its arithmetic lives here.
    let known = crate::classes::classes(ruby)?;
    let bounds = known.bounds.get(ruby);
    bounds.define_method("reach", method!(bounds_reach, 2))?;
    let reach = known.reach.get(ruby);
    reach.define_method("include?", method!(reach_contains, 1))?;
    // The Data classes are defined in Ruby; the core validates them here. Each
    // hook answers the core value (discarded) or raises.
    module.define_singleton_method("__reach", function!(validate_reach, 2))?;
    module.define_singleton_method("__bounds", function!(validate_bounds, 2))?;
    module.define_singleton_method("__pair", function!(validate_pair, 3))?;
    module.define_singleton_method("__record", function!(validate_record, 3))?;
    module.define_singleton_method("__retired", function!(validate_retired, 2))?;
    module.define_singleton_method("__watermark", function!(validate_watermark, 1))?;
    let class = module.define_class("Join", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 1))?;
    class.define_method("bounds", method!(Wrapper::bounds, 0))?;
    class.define_method("capacity", method!(Wrapper::capacity, 1))?;
    class.define_method("size", method!(Wrapper::size, 1))?;
    class.define_method("empty?", method!(Wrapper::empty, 0))?;
    class.define_method("include?", method!(Wrapper::include, 2))?;
    class.define_method("watermark", method!(Wrapper::watermark, 1))?;
    class.define_method("side_watermark", method!(Wrapper::side_watermark, 1))?;
    class.define_method("insert", method!(Wrapper::insert, 2))?;
    class.define_method("pending?", method!(Wrapper::pending, 0))?;
    class.define_method("pending_pairs", method!(Wrapper::pending_pairs, 0))?;
    class.define_method("drain", method!(Wrapper::drain, 1))?;
    class.define_method("advance_watermark", method!(Wrapper::advance_watermark, 2))?;
    class.define_method(
        "advance_watermark_from",
        method!(Wrapper::advance_watermark_from, 2),
    )?;
    class.define_method("retirable", method!(Wrapper::retirable, 0))?;
    class.define_method("retire", method!(Wrapper::retire, 1))?;
    Ok(())
}
