//! Private native entry point.
use crate::{CAPACITY, Wrapper, bounds_reach, other};
use magnus::{Error, RClass, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("JoinMachines")?;
    module.const_set("CAPACITY", CAPACITY)?;
    module.define_singleton_method("other", function!(other, 1))?;
    // Bounds is a Data class defined in Ruby; its arithmetic lives here.
    let bounds: RClass = module.const_get("Bounds")?;
    bounds.define_private_method("native_reach", method!(bounds_reach, 2))?;
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
