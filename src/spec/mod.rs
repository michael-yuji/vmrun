mod decoding;
mod defaults;
mod util;

use crate::spec::util::PciSlotGenerator;
use crate::util::{parse_mem_in_kb, vec_sequence_map};
use crate::vm::{CpuSpec, EmulatedPciDevice, LpcDevice, PciSlot, UefiBoot, VmRun};

use decoding::Emulation;
use serde::{de, Deserialize, Deserializer};
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

use defaults::*;

#[derive(Error, Debug)]
pub enum FormatError {
    #[error("Invalid storage unit {0}")]
    InvalidUnit(String),

    #[error("Invalid value {0}")]
    InvalidValue(std::num::ParseIntError),

    #[error("Invalid format for pci slot. Expected: \"$bus:$slot:$func\". Got: \"{0}\"")]
    InvalidPciSlotRepr(String),

    #[error("Invalid value({value}) for {component} in PCI slot. Max: {max}")]
    PciSlotValueOverflow {
        component: &'static str,
        value: u8,
        max: u8,
    },

    #[error("Cannot find a slot on bus 0 for lpc")]
    LpcSlotNotSatisfy,

    #[error("Cannot find a slot on bus 0 for hostbridge")]
    HostbridgeSlotNotSatisfy,

    #[error("Run out of all Pcie slots")]
    RunOutOfSlots,

    #[error("Selected target not found")]
    ProfileNotFound,
}

fn yes() -> bool {
    true
}
fn no() -> bool {
    false
}

fn empty_hashmap() -> HashMap<String, VmSpecMod> {
    HashMap::new()
}

#[derive(Deserialize, Debug, Clone)]
pub struct VmSpec {
    /* local options */
    pub cpu: CpuSpec,
    pub mem: MemorySpec,

    #[serde(flatten)]
    pub bootopt: Option<BootOptions>,

    pub emulations: Vec<Emulation>,
    pub name: String,

    #[serde(default = "default_hostbridge")]
    pub hostbridge: String,

    pub lpc_slot: Option<PciSlot>,

    /* currently bhyve supports only up to 4 console ports,
     * if we implement a general N console port model, the model may ended up
     * pretty ugly with the extra nest levels and require extra effort for
     * sanity checks and adding error messages.
     * 8 lines is probably a good trade off.
     */
    /// TODO: implement checks
    pub com1: Option<String>,
    pub com2: Option<String>,
    pub com3: Option<String>,
    pub com4: Option<String>,

    /// TODO: implement check
    pub gdb: Option<String>,

    pub uuid: Option<String>,

    pub graphic: Option<GraphicOption>,

    /* yes no flags */
    #[serde(default = "yes")]
    pub utc_clock: bool,

    #[serde(default = "yes")]
    pub yield_on_hlt: bool,

    #[serde(default = "yes")]
    pub generate_acpi: bool,

    #[serde(default = "no")]
    pub wire_guest_mem: bool,

    #[serde(default = "no")]
    pub force_msi: bool,

    #[serde(default = "no")]
    pub disable_mptable_gen: bool,

    #[serde(default = "no")]
    pub power_off_destroy_vm: bool,

    pub extra_options: Option<String>,

    #[serde(default = "empty_hashmap")]
    pub targets: HashMap<String, VmSpecMod>,

    pub next_target: Option<String>,

    pub post_start_script: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct VmSpecMod {
    pub cpu: Option<CpuSpec>,
    pub mem: Option<MemorySpec>,
    #[serde(flatten)]
    pub bootopt: Option<BootOptions>,
    pub emulations: Vec<Emulation>,
    pub gdb: Option<String>,
    pub com1: Option<String>,
    pub com2: Option<String>,
    pub com3: Option<String>,
    pub com4: Option<String>,
    pub utc_clock: Option<bool>,
    pub yield_on_hlt: Option<bool>,
    pub generate_acpi: Option<bool>,
    pub wire_guest_mem: Option<bool>,
    pub force_msi: Option<bool>,
    pub disable_mptable_gen: Option<bool>,
    pub extra_options: Option<String>,
    pub next_target: Option<String>,
    pub post_start_script: Option<String>,
    pub graphic: Option<GraphicOption>,
}

macro_rules! replace_if_some {
    ($self:expr, $other:expr, $field:ident) => {
        if let Some(value) = &$other.$field {
            $self.$field = value.clone();
        }
    };
    ($self:expr, $other:expr, ?$field:ident) => {
        if $other.$field.is_some() {
            $self.$field = $other.$field.clone();
        }
    };
}

impl VmSpec {
    pub fn consume(&mut self, patch: &VmSpecMod) {
        replace_if_some!(self, patch, cpu);
        replace_if_some!(self, patch, mem);
        replace_if_some!(self, patch, ?bootopt);
        replace_if_some!(self, patch, ?gdb);
        replace_if_some!(self, patch, ?com1);
        replace_if_some!(self, patch, ?com2);
        replace_if_some!(self, patch, ?com3);
        replace_if_some!(self, patch, ?com4);

        replace_if_some!(self, patch, utc_clock);
        replace_if_some!(self, patch, yield_on_hlt);
        replace_if_some!(self, patch, generate_acpi);
        replace_if_some!(self, patch, wire_guest_mem);
        replace_if_some!(self, patch, force_msi);
        replace_if_some!(self, patch, disable_mptable_gen);
        replace_if_some!(self, patch, ?extra_options);
        replace_if_some!(self, patch, ?next_target);
        replace_if_some!(self, patch, ?post_start_script);
        replace_if_some!(self, patch, ?graphic);

        self.emulations.extend(patch.emulations.clone());
    }

    #[allow(dead_code)]
    pub fn consumed(&self, patch: &VmSpecMod) -> Self {
        let mut clone = self.clone();
        clone.consume(patch);
        clone
    }

    pub fn build(&self, extra_opts: &[String]) -> Result<VmRun, FormatError> {
        let mut argv: Vec<String> = Vec::new();

        let mut emus: Vec<crate::vm::EmulatedPciDevice> = Vec::new();
        let mut lpcs: Vec<crate::vm::LpcDevice> = Vec::new();

        /* slots explicitly specified in the configuration */
        let mut slots_taken: Vec<PciSlot> = self.emulations.iter().filter_map(|e| e.slot).collect();

        if let Some(lpc_slot) = self.lpc_slot {
            slots_taken.push(lpc_slot);
        }

        let mut slot_gen = PciSlotGenerator::build(0, 0, slots_taken);

        let hostbdg_slot = slot_gen
            .try_take_specific_bus(0)
            .ok_or(FormatError::HostbridgeSlotNotSatisfy)?;

        /* try to stick with convention to put lpc in 0,31, but when it is
         * unavailable, we fetch the next slot available in bus.
         */
        let lpc_slot = match self.lpc_slot {
            None => slot_gen
                .try_take_specific_bus_slot(0, 31)
                .ok_or(FormatError::LpcSlotNotSatisfy)?,
            Some(slot) => slot,
        };

        let bootopt = self.bootopt.clone().unwrap_or_else(default_bootopt);

        match bootopt {
            BootOptions::Uefi(UefiBoot { bootrom, varfile }) => {
                lpcs.push(LpcDevice::Bootrom(bootrom, varfile))
            }
        };

        let mut extra_options = if let Some(opts) = &self.extra_options {
            opts.split(' ')
                .filter_map(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    }
                })
                .collect()
        } else {
            vec![]
        };

        extra_options.extend(extra_opts.to_owned());

        for emulation in &self.emulations {
            let the_slot = if let Some(slot) = emulation.slot {
                Ok(slot)
            } else {
                slot_gen.next_slot().ok_or(FormatError::RunOutOfSlots)
            }?;

            emus.push(crate::vm::EmulatedPciDevice {
                slot: the_slot,
                want_fix: emulation.fix,
                emulation: emulation.to_vm_emu()?,
            });
        }

        if let Some(com) = &self.com1 {
            lpcs.push(LpcDevice::Com(1, com.to_string()));
        }

        if let Some(com) = &self.com2 {
            lpcs.push(LpcDevice::Com(2, com.to_string()));
        }

        if let Some(com) = &self.com3 {
            lpcs.push(LpcDevice::Com(3, com.to_string()));
        }

        if let Some(com) = &self.com4 {
            lpcs.push(LpcDevice::Com(4, com.to_string()));
        }

        if let Some(graphic) = &self.graphic {
            let slot = slot_gen.next_slot().ok_or(FormatError::RunOutOfSlots)?;
            emus.push(EmulatedPciDevice {
                slot,
                want_fix: false,
                emulation: Box::new(graphic.to_emulated()),
            });

            if graphic.xhci_table {
                let slot = slot_gen.next_slot().ok_or(FormatError::RunOutOfSlots)?;
                emus.push(EmulatedPciDevice {
                    want_fix: false,
                    slot,
                    emulation: Box::new(crate::vm::emulation::Xhci {}),
                })
            }
        }

        argv.push(self.name.clone());

        Ok(crate::vm::VmRun {
            cpu: self.cpu,
            mem_kb: self.mem.kb,
            hostbridge_slot: hostbdg_slot,
            hostbridge_brand: self.hostbridge.to_string(),
            lpc_slot,
            lpc_devices: lpcs,
            emulations: emus,
            name: self.name.to_string(),
            uuid: self.uuid.clone(),
            gdb: self.gdb.clone(),
            utc_clock: self.utc_clock,
            yield_on_hlt: self.yield_on_hlt,
            generate_acpi: self.generate_acpi,
            wire_guest_mem: self.wire_guest_mem,
            force_msi: self.force_msi,
            disable_mptable_gen: self.disable_mptable_gen,
            power_off_destroy_vm: self.power_off_destroy_vm,
            extra_options,
        })
    }

    pub fn has_target(&self, target: &String) -> bool {
        self.targets.contains_key(target)
    }

    #[allow(dead_code)]
    pub fn with_target(&self, target: &String) -> Result<Self, FormatError> {
        let modification = self
            .targets
            .get(target)
            .ok_or(FormatError::ProfileNotFound)?;
        Ok(self.consumed(modification))
    }

    pub fn consume_target(&mut self, target: &String) -> Result<(), FormatError> {
        let modification = self
            .targets
            .get(target)
            .ok_or(FormatError::ProfileNotFound)?
            .clone();
        self.consume(&modification);
        Ok(())
    }
}

/// Right biased `Either`, like in Scala and Haskell
/// When deserialize, Right will be prioritized, and only if
/// Deserialize as Right failed, Left will be deserialized
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Either<L, R> {
    Right(R),
    Left(L),
}

#[derive(Deserialize)]
#[serde(remote = "UefiBoot")]
struct UefiBootDef {
    bootrom: String,
    varfile: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct GraphicOption {
    host: String,
    port: Option<u16>,
    vga: Option<String>,
    password: Option<String>,
    #[serde(default = "no")]
    wait: bool,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default = "yes")]
    xhci_table: bool,
}

impl GraphicOption {
    fn to_emulated(&self) -> crate::vm::emulation::Framebuffer {
        crate::vm::emulation::Framebuffer {
            host: self.host.to_string(),
            port: self.port,
            vga: self.vga.clone(),
            password: self.password.clone(),
            w: self.width,
            h: self.height,
            wait: self.wait,
        }
    }
}

impl PciSlot {
    #[allow(dead_code)]
    fn from_bhyve_vpci_slot(s: &str) -> Result<PciSlot, FormatError> {
        let pci = PciSlot::from_str(s)?;
        if pci.slot > 31 {
            Err(FormatError::PciSlotValueOverflow {
                component: "slot",
                value: pci.slot,
                max: 31,
            })
        } else if pci.func > 7 {
            Err(FormatError::PciSlotValueOverflow {
                component: "func",
                value: pci.slot,
                max: 7,
            })
        } else {
            Ok(pci)
        }
    }
}

impl FromStr for PciSlot {
    type Err = FormatError;
    fn from_str(s: &str) -> Result<PciSlot, Self::Err> {
        let comps: Vec<&str> = s.split(':').collect();
        if comps.len() > 3 || comps.len() == 2 {
            Err(FormatError::InvalidPciSlotRepr(s.to_string()))
        } else {
            let nums = vec_sequence_map(&comps, |m| {
                m.parse()
                    .map_err(|_| FormatError::InvalidPciSlotRepr(s.to_string()))
            })?;

            Ok(match nums.len() {
                1 => PciSlot {
                    bus: 0,
                    slot: nums[0],
                    func: 0,
                },
                2 => PciSlot {
                    bus: 0,
                    slot: nums[0],
                    func: nums[1],
                },
                3 => PciSlot {
                    bus: nums[0],
                    slot: nums[1],
                    func: nums[2],
                },
                _ => todo!(), /* unreachable */
            })
        }
    }
}

impl<'de> Deserialize<'de> for PciSlot {
    fn deserialize<D>(deserializer: D) -> Result<PciSlot, D::Error>
    where
        D: Deserializer<'de>,
    {
        let str_value: String = String::deserialize(deserializer)?;
        PciSlot::from_str(str_value.as_str()).map_err(de::Error::custom)
    }
}

impl CpuSpec {
    pub fn from_flat(threads: usize) -> CpuSpec {
        CpuSpec {
            threads,
            cores: 1,
            sockets: 1,
        }
    }
}

impl<'de> Deserialize<'de> for CpuSpec {
    fn deserialize<D>(deserializer: D) -> Result<CpuSpec, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ProxyCpuSpec {
            threads: usize,
            cores: usize,
            sockets: usize,
        }

        Either::<usize, ProxyCpuSpec>::deserialize(deserializer).map(|either| match either {
            Either::Left(threads) => CpuSpec::from_flat(threads),
            Either::Right(spec) => CpuSpec {
                threads: spec.threads,
                cores: spec.cores,
                sockets: spec.sockets,
            },
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MemorySpec {
    pub kb: usize,
}

impl<'de> Deserialize<'de> for MemorySpec {
    fn deserialize<D>(deserializer: D) -> Result<MemorySpec, D::Error>
    where
        D: Deserializer<'de>,
    {
        Either::<usize, String>::deserialize(deserializer).and_then(|either| match either {
            Either::Left(num) => Ok(MemorySpec { kb: num / 1000 }),
            Either::Right(str_value) => {
                let kb = parse_mem_in_kb(&str_value).map_err(de::Error::custom)?;
                Ok(MemorySpec { kb })
            }
        })
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum BootOptions {
    #[serde(with = "UefiBootDef")]
    Uefi(UefiBoot),
}
