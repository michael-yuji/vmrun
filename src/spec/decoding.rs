use crate::spec::FormatError;
use paste::paste;
use serde::{Deserialize, Deserializer};

use crate::vm::emulation::{
    AhciCd, AhciHd, Nvme, NvmeBackend, PciPassthru, VirtioBlk, VirtioConsole, VirtioNet,
};
use crate::vm::{EmulatedPci, NetBackend, PciSlot, RawEmulatedPci};

fn hmap_to_virtio_net<'de, D>(
    hmap: std::collections::HashMap<String, String>,
) -> Result<VirtioNet, D::Error>
where
    D: Deserializer<'de>,
{
    let backend = hmap
        .get(&"name".to_string())
        .map(|s| s.as_str())
        .ok_or_else(|| serde::de::Error::missing_field("name"))?;

    let tpe = match hmap.get(&"type".to_string()) {
        None => {
            if backend.starts_with("tap") {
                Ok(NetBackend::Tap)
            } else if backend.starts_with("netgraph") {
                Ok(NetBackend::Netgraph)
            } else if backend.starts_with("netmap") {
                Ok(NetBackend::Netmap)
            } else if backend.starts_with("vale") {
                Ok(NetBackend::Vale)
            } else {
                Err(serde::de::Error::unknown_variant(
                    backend,
                    &["tap*", "netgraph*", "netmap*", "vale*"],
                ))
            }
        }
        Some(tpe) => match tpe.as_str() {
            "tap" => Ok(NetBackend::Tap),
            "netgraph" => Ok(NetBackend::Netgraph),
            "netmap" => Ok(NetBackend::Netmap),
            "vale" => Ok(NetBackend::Vale),
            _ => Err(serde::de::Error::unknown_variant(
                tpe.as_str(),
                &["tap", "netgraph", "netmap", "vale"],
            )),
        },
    }?;

    let mtu = hmap
        .get(&"mtu".to_string())
        .and_then(|s| s.parse::<u32>().ok());
    let mac = hmap.get(&"mac".to_string()).map(|s| s.to_string());

    Ok(VirtioNet {
        tpe,
        name: backend.to_string(),
        mtu,
        mac,
    })
}

impl<'de> Deserialize<'de> for VirtioNet {
    fn deserialize<D>(deserializer: D) -> Result<VirtioNet, D::Error>
    where
        D: Deserializer<'de>,
    {
        type Hmap = std::collections::HashMap<String, String>;
        let hmap = Hmap::deserialize(deserializer)?;
        hmap_to_virtio_net::<'de, D>(hmap)
    }
}

#[derive(Deserialize)]
#[serde(remote = "crate::vm::emulation::VirtioBlk")]
pub struct VirtioBlkDef {
    pub path: String,
    #[serde(default = "crate::spec::no")]
    pub nocache: bool,
    #[serde(default = "crate::spec::no")]
    pub direct: bool,
    #[serde(default = "crate::spec::no")]
    pub ro: bool,
    #[serde(default = "crate::spec::no")]
    pub nodelete: bool,
    pub logical_sector_size: Option<u32>,
    pub physical_sector_size: Option<u32>,
}

macro_rules! impl_ahci {
    ($name:ident) => {
        paste! {
            #[derive(Deserialize)]
            #[serde(remote = "crate::vm::emulation::" $name)]
            pub struct [<$name Def>] {
                pub path: String,
                pub nmrr:  Option<u32>,
                pub ser:   Option<String>,
                pub rev:   Option<String>,
                pub model: Option<String>
            }
        }
    };
}

impl_ahci!(AhciHd);
impl_ahci!(AhciCd);

impl<'de> Deserialize<'de> for Nvme {
    fn deserialize<D>(deserializer: D) -> Result<Nvme, D::Error>
    where
        D: Deserializer<'de>,
    {
        type Hmap = std::collections::HashMap<String, serde_json::Value>;
        let hmap = Hmap::deserialize(deserializer)?;
        hmap_to_nvme::<'de, D>(hmap)
    }
}

fn hmap_to_nvme<'de, D>(
    hmap: std::collections::HashMap<String, serde_json::Value>,
) -> Result<crate::vm::emulation::Nvme, D::Error>
where
    D: Deserializer<'de>,
{
    macro_rules! take_str {
        ($name:ident) => {
            hmap.get(&stringify!($name).to_string())
                .and_then(|v| v.as_str())
                .ok_or_else::<D::Error, _>(|| serde::de::Error::missing_field(stringify!($name)))
        };
    }
    macro_rules! take_uint {
        ($name:ident, $t:ty) => {
            hmap.get(&stringify!($name).to_string())
                .and_then(|v| v.as_u64())
                .map(|v| v as $t)
                .ok_or_else::<D::Error, _>(|| serde::de::Error::missing_field(stringify!($name)))
        };
    }

    let qsz: Option<u32> = take_uint!(qsz, u32).ok();
    let ioslots = take_uint!(ioslots, u32).ok();
    let sectsz = take_uint!(sectsz, u32).ok();
    let ser = take_str!(ser).ok().map(|s| s.to_string());
    let eui64 = take_uint!(eui64, u32).ok();
    let dsm = take_str!(dsm).ok().map(|s| s.to_string());

    match take_uint!(ram, usize) {
        Ok(value) => Ok(Nvme {
            qsz,
            ioslots,
            sectsz,
            ser,
            eui64,
            dsm,
            backend: NvmeBackend::Ram(value),
        }),
        Err(_) => {
            let path = take_str!(path)?;
            Ok(Nvme {
                qsz,
                ioslots,
                sectsz,
                ser,
                eui64,
                dsm,
                backend: NvmeBackend::Path(path.to_string()),
            })
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(remote = "crate::vm::emulation::VirtioConsole")]
pub struct VirtioConsoleDef {
    ports: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PciPassthruX {
    src: Option<PciSlot>,
    lookup: Option<PciLookup>,
    rom: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct PciLookup {
    device: String,
    vendor: String,
}

impl PciPassthruX {
    fn to_pci_passthru(self) -> Option<PciPassthru> {
        match self.src {
            Some(src) => Some(PciPassthru { src, rom: self.rom }),
            None => {
                if self.lookup.is_none() {
                    None
                } else {
                    let lookup = self.lookup.unwrap();
                    let vendor = lookup.vendor;
                    let device = lookup.device;

                    if vendor.chars().count() != 10 || device.chars().count() != 10 {
                        None
                    } else {
                        let vendor = u32::from_str_radix(&vendor[2..], 16).ok()?;
                        let device = u32::from_str_radix(&device[2..], 16).ok()?;

                        let v1 = ((vendor & 0xffff0000) >> 16) as u16;
                        let v2 = (vendor & 0x0000ffff) as u16;

                        let d1 = ((device & 0xffff0000) >> 16) as u16;
                        let d2 = (device & 0x0000ffff) as u16;

                        println!("v1: {v1:x?}, v2: {v2:x?}, d1: {d1:x?}, d2: {d2:x?}");

                        let devices = crate::util::os::pci::PciDevice::from_pciconf_l();

                        for device in devices.iter() {
                            if device.vendor == v1
                                && device.subvendor == v2
                                && device.device == d1
                                && device.subdevice == d2
                            {
                                return Some(PciPassthru {
                                    src: device.slot,
                                    rom: self.rom,
                                });
                            }
                        }

                        None
                    }
                }
            }
        }
    }
}

/*
#[derive(Deserialize, Debug)]
#[serde(remote = "crate::vm::emulation::PciPassthru")]
pub struct PciPassthruDef {
    src: PciSlot,
    rom: Option<String>
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct FindPci {
    vendor: String,
    device: String
}
*/

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "device")]
pub enum Emulations {
    #[serde(rename = "virtio-console")]
    #[serde(with = "VirtioConsoleDef")]
    VirtioConsole(VirtioConsole),

    #[serde(rename = "virtio-net")]
    VirtioNet(VirtioNet),

    #[serde(with = "VirtioBlkDef")]
    #[serde(rename = "virtio-blk")]
    VirtioBlk(VirtioBlk),

    #[serde(rename = "ahci-hd")]
    #[serde(with = "AhciHdDef")]
    AhciHd(AhciHd),

    #[serde(with = "AhciCdDef")]
    #[serde(rename = "ahci-cd")]
    AhciCd(AhciCd),

    #[serde(rename = "passthru")]
    Passthru(PciPassthruX),

    #[serde(rename = "nvme")]
    Nvme(Nvme),

    #[serde(rename = "raw")]
    Raw { value: String },
}

fn serde_default_emulation_fix() -> bool {
    false
}

#[derive(Deserialize, Debug, Clone)]
pub struct Emulation {
    pub slot: Option<PciSlot>,
    #[serde(default = "serde_default_emulation_fix")]
    pub fix: bool,
    #[serde(flatten)]
    pub emulation: Emulations,
}

impl Emulation {
    pub fn to_vm_emu(&self) -> Result<Box<dyn EmulatedPci>, FormatError> {
        match &self.emulation {
            Emulations::VirtioBlk(x) => Ok(Box::new(x.clone())),
            Emulations::VirtioNet(x) => Ok(Box::new(x.clone())),
            Emulations::AhciCd(x) => Ok(Box::new(x.clone())),
            Emulations::AhciHd(x) => Ok(Box::new(x.clone())),
            Emulations::VirtioConsole(x) => Ok(Box::new(x.clone())),
            Emulations::Nvme(x) => Ok(Box::new(x.clone())),
            Emulations::Passthru(x) => match x.clone().to_pci_passthru() {
                None => Err(FormatError::InvalidUnit("".to_string())),
                Some(passthru) => Ok(Box::new(passthru)),
            },

            //Ok(Box::new(x.clone())),
            Emulations::Raw { value } => Ok(Box::new(RawEmulatedPci {
                value: value.to_string(),
            })),
        }
    }
}
