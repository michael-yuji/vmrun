
use serde::{Deserialize, Deserializer};
use std::str::FromStr;
use crate::spec::FormatError;
use paste::paste;

use crate::vm::{RawEmulatedPci, EmulatedPci, PciSlot, NetBackend};
use crate::vm::emulation::{
    VirtioNet,
    VirtioBlk, 
    VirtioConsole, 
    AhciHd, 
    AhciCd
};

fn hmap_to_virtio_net<'de, D>(hmap: std::collections::HashMap<String, String>) 
    -> Result<VirtioNet, D::Error>
    where D: Deserializer<'de>
{
    let backend = hmap.get(&"name".to_string()).map(|s| s.as_str())
        .ok_or(serde::de::Error::missing_field("name"))?;

    let tpe = match hmap.get(&"type".to_string()) {
        None =>
            if backend.starts_with("tap") {
                Ok(NetBackend::Tap)
            } else if backend.starts_with("netgraph") {
                Ok(NetBackend::Netgraph)
            } else if backend.starts_with("netmap") {
                Ok(NetBackend::Netmap)
            } else {
                Err(serde::de::Error::unknown_variant(
                        backend, &["tap*", "netgraph*", "netmap*"]))
            },
        Some(tpe) =>
            match tpe.as_str() {
                "tap"      => Ok(NetBackend::Tap),
                "netgraph" => Ok(NetBackend::Netgraph),
                "netmap"   => Ok(NetBackend::Netmap),
                _          => 
                    Err(serde::de::Error::unknown_variant(tpe.as_str(), &["tap", "netgraph", "netmap"]))
            }
    }?;

    let mtu = hmap.get(&"mtu".to_string()).and_then(|s| s.parse::<u32>().ok());
    let mac = hmap.get(&"mac".to_string()).map(|s| s.to_string());

    Ok(VirtioNet {
        tpe,
        name: backend.to_string(),
        mtu,
        mac
    })

}

impl<'de> Deserialize<'de> for VirtioNet
{
    fn deserialize<D>(deserializer: D) -> Result<VirtioNet, D::Error>
        where D: Deserializer<'de>
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
                pub path: String,
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
