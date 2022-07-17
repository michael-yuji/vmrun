
use crate::vm::UefiBoot;
use crate::spec::BootOptions;

pub fn default_bootopt() -> BootOptions {
    BootOptions::Uefi(UefiBoot {
        bootrom: "/usr/local/share/uefi-firmware/BHYVE_UEFI.fd".to_string(),
        varfile: None
    })
}

pub fn default_hostbridge() -> String {
    "hostbridge".to_string()
}
