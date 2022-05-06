use std::cmp::Ordering;
use std::str::FromStr;
use thiserror::Error;

pub mod emulation;

type Result<T> = std::result::Result<T, VmError>;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum VmError {
    #[error("The given emulation value({0}) is malformed")]
    MalformedEmulationSyntax(String),
    #[error("lpc interface can only configure on pci bus 0")]
    InvalidLpcEmulation,
    #[error("Requested lpc device but no lpc emulation configured")]
    NoLpc,
    #[error("IOError: {0}")]
    IOError(std::io::Error)
}

pub struct VmRun {
    pub cpu: CpuSpec,

    pub mem_kb: usize,

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

impl VmRun {

    pub fn requirements(&self) -> Vec<Requirement> {
        self.emulations.clone().into_iter()
            .map(|e| e.emulation.requirements().into_iter()).flatten().collect()
    }

    pub fn bhyve_args(&self) -> Result<Vec<String>> {
        let mut argv: Vec<String> = Vec::new();

        let mut has_lpc = false;

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

        push_arg_pair("-c", self.cpu.to_bhyve_arg());
        push_arg_pair("-m", format!("{}K", self.mem_kb));

        if let Some(gdb) = &self.gdb {
            push_arg_pair("-G", gdb.to_string());
        }

        if let Some(uuid) = &self.uuid {
            push_arg_pair("-U", uuid.to_string());
        }

        for emulation in self.emulations.iter() {
            /* need to check if lpc need to be unique? */
            if emulation.is_lpc() {
                has_lpc = true;
            }
            push_arg_pair("-s", emulation.to_bhyve_arg());
        }

        if !(has_lpc || self.lpc_devices.is_empty()) {
            return Err(VmError::NoLpc);
        }

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
pub enum LpcDevice {
    Com(u8, String),
    Bootrom(String, Option<String>),
    TestDev
}

impl LpcDevice {
    fn to_bhyve_arg(&self) -> String {
        match self {
            LpcDevice::Com(i, val) => format!("com{},{}", i, val),
            LpcDevice::TestDev     => format!("pc-testdev"),
            LpcDevice::Bootrom(firmware, varfile) => {
                match varfile {
                    Some(var) => format!("bootrom,{},{}", firmware, var),
                    None      => format!("bootrom,{}", firmware)
                }
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resource {
    Iface(String),
    FsItem(String)
}

impl Resource {
    #[allow(unused_doc_comments)]
    pub fn exists(&self) -> bool {
        match self {
            Resource::FsItem(path) => std::path::Path::new(path.as_str()).exists(),
            /// TODO: Handle network interface existence logic
            Resource::Iface(_)     => true
        }
    }

    pub fn release(&self) -> Result<()> {
        match self {
            Resource::FsItem(path) => 
                std::fs::remove_file(path).map_err(|e| VmError::IOError(e)),
            _ => Ok(())
        }
    }
}

impl std::string::ToString for Resource {
    fn to_string(&self) -> String {
        match self {
            Resource::Iface(iface) => format!("network interface: ({})",iface),
            Resource::FsItem(path) => format!("file: ({})", path)
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum CleanupSemantic {
    /// If the resource is created by the VM, such resource should be removed
    /// after the VM exits.
    Borrow,
    /// Always remove the resource regardless if it exists before the VM boot
    Always,
    /// Never cleanup
    Never
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Requirement {
    ExistsResource(Resource),
    Nonexistence(Resource),
    MayCreate(Resource, CleanupSemantic)
}

impl Requirement {
    pub fn warning(&self) -> String {
        match self {
            Requirement::ExistsResource(resource) => 
                format!("Require existence of {}", resource.to_string()),
            Requirement::Nonexistence(resource)   => 
                format!("Require nonexistence of {}", resource.to_string()),
            Requirement::MayCreate(resource, _) =>
                format!("May create {}", resource.to_string())
        }
    }

    pub fn is_satisfied(&self) -> bool {
        match self {
            Requirement::ExistsResource(res) => res.exists(),
            Requirement::Nonexistence(res)   => !res.exists(),
            _ => true
        }
    }
}

pub trait EmulatedPci: Sized {
    fn as_raw(&self) -> RawEmulatedPci;
    fn requirements(&self) -> Vec<Requirement> {
        vec![]
    }
}

#[derive(Debug, Clone)]
pub enum EmulationOption {
    On(String),
    KeyValue(String, String)
}

#[derive(Debug, Clone)]
pub struct RawEmulatedPci {
    pub frontend: String,
    pub backend:  Option<String>,
    pub options:  Vec<EmulationOption>
}

impl EmulatedPci for RawEmulatedPci {
    fn as_raw(&self) -> RawEmulatedPci {
        self.clone()
    }
}

impl EmulatedPciDevice {
    fn to_bhyve_arg(&self) -> String {
        let mut ret = 
            format!("{},{}", self.slot.to_bhyve_arg(), self.emulation.frontend);

        if let Some(backend) = &self.emulation.backend {
            ret.extend(format!(",{}", backend).chars());
        }

        for option in self.emulation.options.iter() {
            let value = match option {
                EmulationOption::On(flag) => format!(",{}", flag),
                EmulationOption::KeyValue(key, value) => format!(",{}={}", key, value)
            };
            ret.extend(value.chars());
        }

        ret
    }
}

impl FromStr for RawEmulatedPci {
    type Err = VmError;
    fn from_str(val: &str) -> Result<RawEmulatedPci> {
        let mut components = val.split(',');
        let mut options = Vec::<EmulationOption>::new();

        let frontend = components.next().ok_or(
            VmError::MalformedEmulationSyntax(val.to_string())
        )?;

        while let Some(value) = components.next() {
            /* try to split the option by =, if the result length is 1, the option
             * is a flag, otherwise, it is a key value
             */
            let mut lookup = value.splitn(2, "=");
            let flag_or_key = lookup.next().ok_or(
                VmError::MalformedEmulationSyntax(val.to_string())
            )?;

            if let Some(val) = lookup.next() {
                options.push(EmulationOption::KeyValue(
                    flag_or_key.to_string(), val.to_string()));
            } else {
                options.push(EmulationOption::On(flag_or_key.to_string()));
            }
        }

        Ok(RawEmulatedPci {
            frontend: frontend.to_string(),
            backend: None,
            options
        })
    }
}

#[derive(Debug, Clone)]
pub struct EmulatedPciDevice {
    pub slot: PciSlot,
    pub emulation: RawEmulatedPci
}

impl EmulatedPciDevice {
    fn is_lpc(&self) -> bool {
        self.emulation.frontend.as_str() == "lpc"
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
    fn to_bhyve_arg(&self) -> String {
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
    pub slot: u8, /* 0-31 */
    pub func: u8  /* 0-7 */
}

impl PciSlot {
    pub fn to_bhyve_arg(&self) -> String {
        format!("{}:{}:{}", self.bus, self.slot, self.func)
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


