extern crate tikv_jemalloc_sys;

#[used]
static INIT: unsafe extern "C" fn(usize) -> *mut std::ffi::c_void = tikv_jemalloc_sys::malloc;
