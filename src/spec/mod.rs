
mod decoding;
mod util;

use crate::spec::util::PciSlotGenerator;
use crate::util::{parse_mem_in_kb, vec_sequence_map};
use crate::vm::{CpuSpec, UefiBoot, PciSlot, EmulatedPciDevice, RawEmulatedPci, LpcDevice, VmRun, EmulatedPci};

use decoding::Emulation;
use serde::{Deserialize, Deserializer, de};
use std::str::FromStr;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FormatError {
    #[error("Invalid storage unit {0}")]
    InvalidUnit(String),

    #[error("Invalid value {0}")]
    InvalidValue(std::num::ParseIntError),

    #[error("Invalid format for pci slot($bus/$slot/$func): {0}")]
    InvalidPciSlotRepr(String),

    #[error("Invalid value({value}) for {component} in PCI slot. Max: {max}")]
    PciSlotValueOverflow { component: &'static str, value: u8, max: u8 },

    #[error("Cannot find a slot on bus 0 for lpc")]
    LpcSlotNotSatisfy,

    #[error("Cannot find a slot on bus 0 for hostbridge")]
    HostbridgeSlotNotSatisfy,

    #[error("Incorrect emulation line")]
    IncorrectEmulation,

    #[error("Run out of all Pcie slots")]
    RunOutOfSlots,

    #[error("Selected target not found")]
    ProfileNotFound
}

fn yes() -> bool { true  }
fn no()  -> bool { false }
fn default_hostbridge() -> String { "hostbridge".to_string() }
fn empty_hashmap() -> HashMap<String, VmSpecMod> {
    HashMap::new()
}

#[derive(Deserialize, Debug, Clone)]
pub struct VmSpec {

    /* local options */

    pub cpu: CpuSpec,
    pub mem: MemorySpec,
    #[serde(flatten)]
    pub bootopt: BootOptions,
    pub emulations: Vec<Emulation>,
    pub name: String,

    #[serde(default = "default_hostbridge")]
    pub hostbridge: String,

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

    pub next_target: Option<String>
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
    pub utc_clock:      Option<bool>,
    pub yield_on_hlt:   Option<bool>,
    pub generate_acpi:  Option<bool>,
    pub wire_guest_mem: Option<bool>,
    pub force_msi:      Option<bool>,
    pub disable_mptable_gen: Option<bool>,
    pub extra_options:       Option<String>,
    pub next_target: Option<String>
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
    }
}

impl VmSpec
{
    pub fn consume(&mut self, patch: &VmSpecMod) {
        replace_if_some!(self, patch, cpu);
        replace_if_some!(self, patch, mem);
        replace_if_some!(self, patch, bootopt);
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

        self.emulations.extend(patch.emulations.clone());
    }

    #[allow(dead_code)]
    pub fn consumed(&self, patch: &VmSpecMod) -> Self {
        let mut clone = self.clone();
        clone.consume(patch);
        clone
    }

    pub fn build(&self) -> Result<VmRun, FormatError>
    {
        let mut argv: Vec<String> = Vec::new();

        let mut emus: Vec<crate::vm::EmulatedPciDevice> = Vec::new();
        let mut lpcs: Vec<crate::vm::LpcDevice> = Vec::new();

        /* slots explicitly specified in the configuration */
        let slots_taken: Vec<PciSlot> = 
            self.emulations.iter().filter_map(|e| e.slot).collect();

        let mut slot_gen = PciSlotGenerator::build(0, 0, slots_taken);

        let hostbdg_slot = slot_gen.try_take_specific_bus(0)
            .ok_or(FormatError::HostbridgeSlotNotSatisfy)?;

        /* try to stick with convention to put lpc in 0,31, but when it is 
         * unavailable, we fetch the next slot available in bus 0; however 
         * if none of the slots are available in bus 0, we need to abort
         * as lpc only works on bus 0
         */
        let lpc_slot = slot_gen.try_take_specific_bus_slot(0, 31)
                         .or_else(|| slot_gen.try_take_specific_bus(0))
                         .ok_or(FormatError::LpcSlotNotSatisfy)?;

        match &self.bootopt {
            BootOptions::Uefi(UefiBoot { bootrom, varfile }) =>
                lpcs.push(LpcDevice::Bootrom(bootrom.to_string(), varfile.clone()))
        };

        let extra_options = 
            if let Some(opts) = &self.extra_options {
                opts.split(' ')
                   .filter_map(|s|{ 
                       if s.len() == 0 { None } else { Some(s.to_string()) }
               }).collect()
        } else { vec![] };


        emus.push(EmulatedPciDevice { 
            slot: hostbdg_slot, 
            emulation: RawEmulatedPci { 
                frontend: "hostbridge".to_string(), 
        device:   "hostbridge".to_string(),
                backend: None, 
                options: vec![]
            }
        });

        emus.push(EmulatedPciDevice { 
            slot: lpc_slot, 
            emulation: RawEmulatedPci { 
                frontend: "lpc".to_string(), 
        device:   "lpc".to_string(),
                backend: None, 
                options: vec![]
            }
        });

        for emulation in &self.emulations {
            let the_slot = 
                if let Some(slot) = emulation.slot {
                    Ok(slot)
                } else {
                    slot_gen.next_slot().ok_or(FormatError::RunOutOfSlots)
                }?;

            emus.push(crate::vm::EmulatedPciDevice {
                slot: the_slot, emulation: emulation.to_vm_emu()? });
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
                emulation: graphic.to_emulated()
            });

            if graphic.xhci_table {
                let slot = slot_gen.next_slot().ok_or(FormatError::RunOutOfSlots)?;
                emus.push(EmulatedPciDevice {
                    slot, emulation: crate::vm::emulation::Xhci { }.as_raw() })
            }
        }

        argv.push(self.name.clone());

        Ok(crate::vm::VmRun {
            cpu: self.cpu.clone(),
            mem_kb: self.mem.kb,
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
            extra_options
        })
    }

    pub fn has_target(&self, target: &String) -> bool {
        self.targets.contains_key(target)
    }

    #[allow(dead_code)]
    pub fn with_target(&self, target: &String) -> Result<Self, FormatError> {
        let modification =
            self.targets.get(target).ok_or(FormatError::ProfileNotFound)?;
        Ok(self.consumed(modification))
    }

    pub fn consume_target(&mut self, target: &String) -> Result<(), FormatError> {
        let modification =
            self.targets.get(target).ok_or(FormatError::ProfileNotFound)?.clone();
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
    Left(L)
}

#[derive(Deserialize)]
#[serde(remote = "UefiBoot")]
struct UefiBootDef {
    bootrom: String,
    varfile: Option<String>
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
    xhci_table: bool
}

impl GraphicOption {
    fn to_emulated(&self) -> RawEmulatedPci {
        crate::vm::emulation::Framebuffer {
            host: self.host.to_string(),
            port: self.port,
            vga: self.vga.clone(),
            password: self.password.clone(),
            w: self.width,
            h: self.height,
            wait: self.wait
        }.as_raw()
    }
}

impl FromStr for PciSlot
{
    type Err = FormatError;

    fn from_str(s: &str) -> Result<PciSlot, Self::Err>
    {
        let comps: Vec<&str> = s.split('/').collect();

        let nums: Vec<u8> = vec_sequence_map(
            &comps, |m| m.parse().map_err(|e| FormatError::InvalidValue(e)))?;

        if comps.len() > 3 || comps.len() == 2 {
            return Err(FormatError::InvalidPciSlotRepr(s.to_string()));
        }

        fn assert_v(component: &'static str, value: u8, max: u8)
            -> Result<u8, FormatError>
        {
            if value > max {
                Err(FormatError::PciSlotValueOverflow { component
                                                      , value
                                                      , max })
            } else {
                Ok(value)
            }
        }

        match nums.len() {
            1 => 
                assert_v("slot", nums[0], 31)
                 .map(|slot| PciSlot { bus: 0, slot, func: 0 }),
            2 => {
                let slot = assert_v("slot", nums[0], 31)?;
                let func = assert_v("func", nums[1], 7)?;
                Ok(PciSlot { bus: 0, slot, func })
            },
            3 => {
                let bus  = assert_v("bus",  nums[0], 255)?;
                let slot = assert_v("slot", nums[1], 31)?;
                let func = assert_v("func", nums[2], 7)?;

                Ok(PciSlot { bus, slot, func })
            }
            _ => todo!() /* impossible */
        }
    }
}

impl <'de> Deserialize<'de> for PciSlot {
    fn deserialize<D>(deserializer: D) -> Result<PciSlot, D::Error>
        where D: Deserializer<'de>
    {
        let str_value: String = String::deserialize(deserializer)?;
        PciSlot::from_str(str_value.as_str()).map_err(de::Error::custom)
    }
}

impl CpuSpec
{
    pub fn from_flat(threads: usize) -> CpuSpec {
        CpuSpec { threads, cores: 1, sockets: 1 }
    }
}

impl<'de> Deserialize<'de> for CpuSpec {
    fn deserialize<D>(deserializer: D) -> Result<CpuSpec, D::Error>  
        where D: Deserializer<'de>
    {
        #[derive(Deserialize)]
        struct ProxyCpuSpec {
            threads: usize,
            cores:   usize,
            sockets: usize
        }


        Either::<usize, ProxyCpuSpec>::deserialize(deserializer).map(|either| {
            match either {
                Either::Left(threads) => CpuSpec::from_flat(threads),
                Either::Right(spec)  =>
                    CpuSpec { threads: spec.threads
                            , cores:   spec.cores
                            , sockets: spec.sockets
                            }
            }
        })
    }
}



#[derive(Debug, Copy, Clone)]
pub struct MemorySpec { pub kb: usize }

impl <'de> Deserialize<'de> for MemorySpec {
    fn deserialize<D>(deserializer: D) -> Result<MemorySpec, D::Error>
        where D: Deserializer<'de>
    {
        Either::<usize, String>::deserialize(deserializer).and_then(|either| {
            match either {
                Either::Left(num) => Ok(MemorySpec { kb: num/1000 }),
                Either::Right(str_value) => {
                    let kb = parse_mem_in_kb(&str_value)
                        .map_err(de::Error::custom)?;
                    Ok(MemorySpec { kb })
                }
            }
        })
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum BootOptions {
    #[serde(with = "UefiBootDef")]
    Uefi(UefiBoot)
}
