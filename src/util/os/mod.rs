pub mod iface;
pub mod pci;

#[link(name = "c")]
extern "C" {
    fn kldfind(file: *const std::os::raw::c_char) -> std::os::raw::c_int;
}

pub fn exists_kld(file: &str) -> Option<bool> {
    unsafe {
        let c_str = std::ffi::CString::new(file).ok()?;
        match kldfind(c_str.as_ptr()) {
            -1 => Some(false),
            _ => Some(true),
        }
    }
}
