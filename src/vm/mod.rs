use std::cmp::Ordering;

use crate::util::assertion::Assertion;
use crate::vm::conditions::{
    Condition, ValidBhyveVPciSlot, Existence, FsEntity, NoCond, 
    NestedConditions, GenericFatalCondition, KernelFeature
};

pub mod emulation;
pub mod conditions;

type Result<T> = std::result::Result<T, Assertion>;

pub trait BhyveDev {
    fn preconditions(&self) -> Box<dyn conditions::Condition>;
}


#[derive(Debug)]
pub struct VmRun
{
    pub cpu: CpuSpec,

    pub mem_kb: usize,

    pub hostbridge_brand: String,

    pub hostbridge_slot: PciSlot,

    pub lpc_slot: PciSlot,

    pub lpc_devices: Vec<LpcDevice>,

    pub emulations: Vec<EmulatedPciDevice>,

    pub name: String,

    pub uuid: Option<String>,

    /// TODO: add constraints and maybe use an extra type to repr it?
    pub gdb: Option<String>,

    pub utc_clock: bool,

    pub yield_on_hlt: bool,

    pub generate_acpi: bool,

    pub wire_guest_mem: bool,

    pub force_msi: bool,

    pub disable_mptable_gen: bool,

    pub power_off_destroy_vm: bool,

    pub extra_options: Vec<String>
}

impl BhyveDev for VmRun {
    fn preconditions(&self) -> Box<dyn Condition> {

        let mut emuc = vec![];
        let mut lpc = vec![];

        let mut lpc_seen = vec![];

        for emulation in self.emulations.iter() {
            emuc.push(emulation.preconditions());
        }
        for lpc_device in self.lpc_devices.iter() {
            let id = lpc_device.identifier();

            // Do not exit early to collect all possible issue with the config
            if lpc_seen.contains(&id) {
                lpc.push(GenericFatalCondition::new_boxed(
                        "duplicated_lpc_device",
                        format!("lpc device {id} are specified more than once").as_str()));
            } else {
                lpc_seen.push(lpc_device.identifier());
            }

            lpc.push(lpc_device.preconditions());
        }

        let nc = Box::new(NestedConditions { name: "vpci".to_string(), conditions: emuc });
        let lc = Box::new(NestedConditions { name: "lpc".to_string(), conditions: lpc });

        Box::new(NestedConditions { name: "vm".to_string(), conditions: vec![nc, lc] })
    }
}


impl VmRun
{
    pub fn ephemeral_objects(&self) -> Vec<Resource> {
        let mut ephemeral_objects = vec![];
        for emulation in self.emulations.iter() {
            ephemeral_objects.extend(emulation.emulation.ephemeral_objects());
        }
        ephemeral_objects
    }

    pub fn bhyve_args(&self) -> Result<Vec<String>>
    {
        let mut argv: Vec<String> = Vec::new();

        self.preconditions().check()?;

        let mut push_yesno = |cond: bool, value: &'static str| {
            if cond { argv.push(value.to_string()) }
        };

        push_yesno(self.generate_acpi,        "-A");
        push_yesno(self.wire_guest_mem,       "-S");
        push_yesno(self.yield_on_hlt,         "-H");
        push_yesno(self.force_msi,            "-W");
        push_yesno(self.disable_mptable_gen,  "-Y");
        push_yesno(self.utc_clock,            "-u");
        push_yesno(self.power_off_destroy_vm, "-D");

        let mut push_arg_pair = |key: &'static str, value: String| {
            argv.push(key.to_string());
            argv.push(value);
        };

        push_arg_pair("-c", self.cpu.as_bhyve_arg());
        push_arg_pair("-m", format!("{}K", self.mem_kb));

        if let Some(gdb) = &self.gdb {
            push_arg_pair("-G", gdb.to_string());
        }

        if let Some(uuid) = &self.uuid {
            push_arg_pair("-U", uuid.to_string());
        }

        push_arg_pair("-s", format!("{},{}", self.hostbridge_slot.as_bhyve_arg(), self.hostbridge_brand));
        push_arg_pair("-s", format!("{},lpc", self.lpc_slot.as_bhyve_arg()));

        for emulation in self.emulations.iter() {
            push_arg_pair("-s", emulation.to_bhyve_arg());
        }

        // This logic is now handled by the precondition checks so we don't have
        // to worry about it, but it is nice to have some reminder of this requirement
        // here
        /*
        if self.lpc_devices.is_empty() {
            return Err(VmError::NoLpc);
        }
        */

        if !self.lpc_devices.is_empty() {
            for lpc in &self.lpc_devices {
                push_arg_pair("-l", lpc.to_bhyve_arg());
            }
        }

        argv.push(self.name.to_string());

        Ok(argv)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum LpcDevice {
    Com(u8, String),
    Bootrom(String, Option<String>),
    TestDev
}

impl LpcDevice {
    fn identifier(&self) -> String {
        match self {
            LpcDevice::Bootrom(..) => "bootrom".to_string(),
            LpcDevice::Com(i, ..) => format!("com{i}"),
            LpcDevice::TestDev => "testdev".to_string()
        }
    }
}

impl BhyveDev for LpcDevice {
    fn preconditions(&self) -> Box<dyn Condition> {
        match self {
            LpcDevice::Bootrom(bootrom, bootvars) => {
                let mut base: Vec<Box<dyn Condition>> = vec![Box::new(Existence { resource: FsEntity::File(std::path::PathBuf::from(bootrom)) })];
                if let Some(bootvars) = bootvars {
                    let vars = std::path::PathBuf::from(bootvars);
                    base.push(Box::new(Existence { resource: FsEntity::File(vars) }));
                }
                
                Box::new(NestedConditions { name: "lpc".to_string(), conditions: base })
            },
            LpcDevice::Com(n, device) => {
                let mut conditions = vec![];

                if let 1u8..=3 = n {} else {
                    conditions.push(GenericFatalCondition::new_boxed(
                        "invalid-com-number",
                        "only com[1-3] are supported"));
                }

                match device.as_str() {
                    "stdio" => (),
                    otherwise => {
                        if otherwise.starts_with("nmdm") {
                            conditions.push(KernelFeature::new_boxed("nmdm"));
                        } else {
                            conditions.push(GenericFatalCondition::new_boxed(
                                "invalid-com-device",
                                "com device must be either stdio or nmdm device"));
                        }
                    }
                }


                Box::new(NestedConditions { name: "lpc".to_string(), conditions })
            }
            _ => Box::new(NoCond {})
        }
    }
}

impl LpcDevice
{
    fn to_bhyve_arg(&self) -> String {
        match self {
            LpcDevice::Com(i, val) => format!("com{},{}", i, val),
            LpcDevice::TestDev     => "pc-testdev".to_string(),
            LpcDevice::Bootrom(firmware, varfile) => {
                match varfile {
                    Some(var) => format!("bootrom,{},{}", firmware, var),
                    None      => format!("bootrom,{}", firmware)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetBackend {
    Tap, Netgraph, Netmap, Vale
}

impl ToString for NetBackend {
    fn to_string(&self) -> String {
        match self {
            NetBackend::Tap => "tap",
            NetBackend::Netmap => "netmap",
            NetBackend::Netgraph => "netgraph",
            NetBackend::Vale => "vale"
        }.to_string()
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resource {
    Iface(NetBackend, String),
    FsItem(String),
    Node(String),
//    PassthruPci(PciSlot)
}

impl Resource {

    pub fn release(&self) -> Result<()> {
        match self {
            Resource::FsItem(path) => 
                std::fs::remove_file(path).map_err(Assertion::from_io_error),
            Resource::Node(node) =>
                std::fs::remove_file(node).map_err(Assertion::from_io_error),
            _ => Ok(())
        }
    }
}

impl std::string::ToString for Resource {
    fn to_string(&self) -> String {
        match self {
            Resource::Iface(tpe, iface) => format!("network interface of type ({}): ({})", tpe.to_string(), iface),
            Resource::FsItem(path) => format!("file: ({})", path),
            Resource::Node(node)   => format!("node: ({})", node),
        }
    }
}

pub trait EmulatedPci: std::fmt::Debug + BhyveDev
{
    fn as_bhyve_arg(&self) -> String;

    fn ephemeral_objects(&self) -> Vec<Resource> {
        vec![]
    }

}

#[derive(Debug, Clone)]
pub struct RawEmulatedPci {
    pub value: String
}

impl BhyveDev for RawEmulatedPci {
    fn preconditions(&self) -> Box<dyn Condition> {
        Box::new(NoCond {})
    }
}

impl EmulatedPci for RawEmulatedPci {
    fn as_bhyve_arg(&self) -> String {
        self.value.to_string()
    }
}

#[derive(Debug)]
pub struct EmulatedPciDevice {
    pub slot: PciSlot,
    pub want_fix: bool,
    pub emulation: Box<dyn EmulatedPci>
}

impl BhyveDev for EmulatedPciDevice {
    fn preconditions(&self) -> Box<dyn Condition> {
        let mut base: Vec<Box<dyn Condition>> = vec![Box::new(ValidBhyveVPciSlot { slot: self.slot })];

        let (bus, slot, func) = (self.slot.bus, self.slot.slot, self.slot.func);
        base.push(self.emulation.preconditions());
        Box::new(NestedConditions
            { name: format!("pci:{bus}:{slot}:{func}")
            , conditions: base
            })
    }
}

impl EmulatedPciDevice {
    fn to_bhyve_arg(&self) -> String {
        format!("{},{}", self.slot.as_bhyve_arg(), self.emulation.as_bhyve_arg())
    }
}

#[derive(Debug, Clone)]
pub struct UefiBoot {
    pub bootrom: String,
    pub varfile: Option<String>
}

#[derive(Debug, Copy, Clone)]
pub struct CpuSpec {
    pub threads: usize,
    pub cores:   usize,
    pub sockets: usize
}

impl CpuSpec {
    fn as_bhyve_arg(&self) -> String {
        if self.sockets == 1 && self.cores == 1 {
            self.threads.to_string()
        } else {
            format!("sockets={},threads={},cores={}", self.sockets, self.cores, self.threads)
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct PciSlot {
    pub bus:  u8,
    pub slot: u8,
    pub func: u8
}

impl PciSlot {
    pub fn as_bhyve_arg(&self) -> String {
        format!("{}:{}:{}", self.bus, self.slot, self.func)
    }

    pub fn as_passthru_arg(&self) -> String {
        format!("{}/{}/{}", self.bus, self.slot, self.func)
    }
}

impl Ord for PciSlot {
    fn cmp(&self, other: &Self) -> Ordering {
        let mut cur_cmp = self.bus.cmp(&other.bus);

        if cur_cmp == Ordering::Equal {
            cur_cmp = self.slot.cmp(&other.slot);

            if cur_cmp == Ordering::Equal {
                cur_cmp = self.func.cmp(&other.func);
            }
        }

        cur_cmp
    }
}

impl PartialOrd for PciSlot {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

