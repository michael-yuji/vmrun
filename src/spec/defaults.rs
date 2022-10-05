use crate::spec::BootOptions;
use crate::vm::UefiBoot;

pub fn default_bootopt() -> BootOptions {
    BootOptions::Uefi(UefiBoot {
        bootrom: "/usr/local/share/uefi-firmware/BHYVE_UEFI.fd".to_string(),
        varfile: None,
    })
}

pub fn default_hostbridge() -> String {
    "hostbridge".to_string()
}
