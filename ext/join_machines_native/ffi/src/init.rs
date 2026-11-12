//! Private native entry point.
use crate::{CAPACITY, Wrapper};
use magnus::{Error, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("JoinMachines")?;
    module.const_set("CAPACITY", CAPACITY)?;
    let class = module.define_class("Join", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_method("insert", method!(Wrapper::insert, 4))?;
    class.define_method("pending?", method!(Wrapper::pending, 0))?;
    class.define_method("pending_pairs", method!(Wrapper::pending_pairs, 0))?;
    class.define_method("drain", method!(Wrapper::drain, 1))?;
    class.define_method("advance_watermark", method!(Wrapper::advance_watermark, 2))?;
    class.define_method("watermark", method!(Wrapper::watermark, 1))?;
    class.define_method("retirable", method!(Wrapper::retirable, 0))?;
    class.define_method("retire", method!(Wrapper::retire, 1))?;
    class.define_method("size", method!(Wrapper::size, 1))?;
    class.define_method("include?", method!(Wrapper::include, 2))?;
    class.define_method("empty?", method!(Wrapper::empty, 0))?;
    class.define_method("capacity", function!(Wrapper::capacity, 0))?;
    class.define_method("before", method!(Wrapper::before, 0))?;
    class.define_method("after", method!(Wrapper::after, 0))?;
    Ok(())
}
