use crate::vm::PciSlot;
use command_macros::cmd;

#[derive(Debug)]
pub struct PciDevice {
    pub device_name: String,
    pub domain: u8,
    pub slot: PciSlot,
    pub class: u32,
    pub rev: u8,
    pub header_type: u8,
    pub vendor: u16,
    pub subvendor: u16,
    pub device: u16,
    pub subdevice: u16,
}
/*
enum PassthruCheck {
    Ok,
    NotPpt(String),
    WrongHdr { hdr: u8, rev: u8, class: u32 }
}
*/
impl PciDevice {
    /*
        fn check_passthru(&self) -> PassthruCheck {
            if !self.device_name.starts_with("ppt") {
                PassthruCheck::NotPpt(self.device_name.to_string())
            } else if self.header_type != 0x00 {
                // Not an end point device, if this value is 0x7f, it could indicate
                // the slot on the motherboard does not support SR-IOV (If the Pci
                // is a VF)
                PassthruCheck::WrongHdr { hdr: self.header_type, rev: self.rev, class: self.class }
            } else {
                PassthruCheck::Ok
            }
        }
        pub fn can_passthru(&self) -> Result<(), &'static str> {
            if !self.device_name.starts_with("ppt") {
                Err("Device is not loaded with ppt driver")
            } else if self.header_type == 0x7f || self.rev == 0xff || self.class == 0xffffff {
                Err("Invalid hdr, class or rev value. If this is a SR-IOV virtual function,
                    Please check if the motherboard supports/enabled SR-IOV and BIOS
                    are set correctly")
            } else {
                Ok(())
            }
        }
    */
    pub fn force_passthru(&mut self) {
        let domain = self.domain;
        let bus = self.slot.bus;
        let slot = self.slot.slot;
        let func = self.slot.func;
        let selector = format!("pci{domain}:{bus}:{slot}:{func}");

        if !self.device_name.starts_with("none") {
            cmd!(devctl detach (selector)).status().unwrap();
        }

        cmd!(devctl set driver (selector) ppt).status().unwrap();

        let refreshed = cmd!(pciconf("-l")(selector)).output().unwrap();
        let dev = PciDevice::from_pciconf_l_line(std::str::from_utf8(&refreshed.stdout).unwrap());

        *self = dev;
    }

    pub fn from_pciconf(slot: &PciSlot) -> Option<PciDevice> {
        let out = cmd!(pciconf("-l")(format!(
            "pci0:{}:{}:{}",
            slot.bus, slot.slot, slot.func
        )))
        .output()
        .unwrap();
        let line = std::str::from_utf8(&out.stdout).ok()?;
        if line.is_empty() {
            None
        } else {
            Some(PciDevice::from_pciconf_l_line(line))
        }
    }

    #[allow(dead_code)]
    pub fn from_pciconf_l() -> Vec<PciDevice> {
        let output = std::process::Command::new("pciconf")
            .arg("-l")
            .output()
            .unwrap();
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        stdout.lines().map(PciDevice::from_pciconf_l_line).collect()
    }

    pub fn from_pciconf_l_line(line: &str) -> PciDevice {
        const ERR: &str = "Invalid pciconf -l output";

        macro_rules! take_next_expected_key {
            ($cols:expr, $key:expr, $t:ty) => {{
                let (key, value) = $cols.next().and_then(|x| x.split_once('=')).expect(ERR);
                if key != $key {
                    panic!("{}", ERR);
                }
                <$t>::from_str_radix(&value[2..], 16).expect(ERR)
            }};
        }

        let mut cols = line.split_whitespace();
        let (name, slot_str) = cols.next().and_then(|x| x.split_once("@pci")).expect(ERR);
        let numstr: Vec<_> = slot_str.split(':').collect();
        let num: Vec<u8> = numstr.iter().filter_map(|s| s.parse::<u8>().ok()).collect();

        let domain = num[0];
        let slot = PciSlot {
            bus: num[1],
            slot: num[2],
            func: num[3],
        };
        let device_name = name.to_string();
        let class = take_next_expected_key!(cols, "class", u32);
        let rev = take_next_expected_key!(cols, "rev", u8);
        let header_type = take_next_expected_key!(cols, "hdr", u8);
        let vendor = take_next_expected_key!(cols, "vendor", u16);
        let device = take_next_expected_key!(cols, "device", u16);
        let subvendor = take_next_expected_key!(cols, "subvendor", u16);
        let subdevice = take_next_expected_key!(cols, "subdevice", u16);

        PciDevice {
            device_name,
            domain,
            slot,
            class,
            rev,
            header_type,
            vendor,
            subvendor,
            device,
            subdevice,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PciDevice;

    #[test]
    fn test_pci_decode() {
        let input = "ppt0@pci0:114:0:0:      class=0x028000 rev=0x1a hdr=0x00 vendor=0x8086 device=0x2725 subvendor=0x8086 subdevice=0x0024";
        let dev = PciDevice::from_pciconf_l_line(input);

        assert_eq!(dev.device_name, "ppt0");
        assert_eq!(dev.class, 0x28000);
        assert_eq!(dev.rev, 0x1a);
        assert_eq!(dev.header_type, 0x00);
        assert_eq!(dev.vendor, 0x8086);
        assert_eq!(dev.device, 0x2725);
        assert_eq!(dev.subvendor, 0x8086);
        assert_eq!(dev.subdevice, 0x0024);
    }
}
