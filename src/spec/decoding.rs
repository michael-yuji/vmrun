
use serde::Deserialize;
use std::str::FromStr;
use crate::spec::FormatError;
use paste::paste;

use crate::vm::{RawEmulatedPci, EmulatedPci, PciSlot};
use crate::vm::emulation::{
    VirtioNet,
    VirtioBlk, 
    VirtioConsole, 
    AhciHd, 
    AhciCd
};

#[derive(Deserialize, Debug)]
#[serde(tag = "backend")]
#[serde(remote = "crate::vm::emulation::VirtioNetBackend")]
pub enum VirtioNetBackendDef {
    #[serde(rename = "tap")]
    Tap,
    #[serde(rename = "netgraph")]
    Netgraph,
    #[serde(rename = "netmap")]
    Netmap
}

#[derive(Deserialize)]
#[serde(remote = "crate::vm::emulation::VirtioNet")]
pub struct VirtioNetDef {
    #[serde(with = "VirtioNetBackendDef")]
    #[serde(flatten)]
    pub tpe:  crate::vm::emulation::VirtioNetBackend,
    pub name: String,
    pub mtu:  Option<u32>,
    pub mac:  Option<String>
}

#[derive(Deserialize)]
#[serde(remote = "crate::vm::emulation::VirtioBlk")]
pub struct VirtioBlkDef {
    pub device: String,
    #[serde(default = "crate::spec::no")]
    pub nocache: bool,
    #[serde(default = "crate::spec::no")]
    pub direct:  bool,
    #[serde(default = "crate::spec::no")]
    pub ro:      bool,
    #[serde(default = "crate::spec::no")]
    pub nodelete: bool,
    pub logical_sector_size: Option<u32>,
    pub physical_sector_size: Option<u32>
}

macro_rules! impl_ahci {
    ($name:ident) => {
        paste! {
            #[derive(Deserialize)]
            #[serde(remote = "crate::vm::emulation::" $name)]
            pub struct [<$name Def>] {
                pub device: String,
                pub nmrr:  Option<u32>,
                pub ser:   Option<String>,
                pub rev:   Option<String>,
                pub model: Option<String>
            }
        }
    }
}

impl_ahci!(AhciHd);
impl_ahci!(AhciCd);

#[derive(Deserialize, Debug)]
#[serde(remote = "crate::vm::emulation::VirtioConsole")]
pub struct VirtioConsoleDef {
    ports: Vec<String>
}

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "frontend")]
pub enum Emulations {
    #[serde(rename = "virtio-console")]
    #[serde(with = "VirtioConsoleDef")]
    VirtioConsole(VirtioConsole),

    #[serde(with = "VirtioNetDef")]
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

    #[serde(rename = "raw")]
    Raw { value: String }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Emulation {
    pub slot: Option<PciSlot>,
    #[serde(flatten)]
    pub emulation: Emulations
}

impl Emulation {
    pub fn to_vm_emu(&self) -> Result<RawEmulatedPci, FormatError> {
        match &self.emulation {
            Emulations::VirtioBlk(x) => Ok(x.as_raw()),
            Emulations::VirtioNet(x) => Ok(x.as_raw()),
            Emulations::AhciCd(x) => Ok(x.as_raw()),
            Emulations::AhciHd(x) => Ok(x.as_raw()),
            Emulations::VirtioConsole(x) => Ok(x.as_raw()),
            Emulations::Raw { value } =>
                RawEmulatedPci::from_str(&value)
                .map_err(|_e| FormatError::IncorrectEmulation)
                .map(|x| x)
        }
    }
}
