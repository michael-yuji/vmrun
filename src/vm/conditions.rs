
use std::path::PathBuf;
use crate::util::assertion::{Recovery, Assertion};
use crate::util::os::iface::{get_tap_ifaces, is_tap_opened};
use crate::util::os::pci::PciDevice;
use crate::vm::{PciSlot, NetBackend};
use command_macros::cmd;

/// A abstract interface to define rules that assert if a system is capable
/// to run the given VM configuration. Given a VM configuration, we can generate
/// a number of `Condition` base on the configuration itself, regardless of the
/// capability and architecture of the host generating it, a list of conditions 
/// can then run on a real enviornment by applying check(). These checks produce
/// a collection of `Assertion` that determine if the given host can launch the VM
///
/// Notice that if there are incorrectness in the configuration itself, a condition
/// that always fail can be used such that the validation always failed (which is
/// semanticly correct, because no host can run an impossible vm). The implication
/// of this is that it is okay to implement conditions are captures the state of
/// the *configuration* by copy in values in the configuration itself.
pub trait Condition: std::fmt::Debug {

    /// Check if this condition is satisfied, and provide failure reason and 
    /// recoverability information
    fn check(&self) -> Result<(), Assertion>;

    /// The name of this condition,
    fn name(&self) -> String;

    fn assert_failure(&self, why: String) -> Result<(), Assertion> {
        Err(Assertion::Fatal(self.name(), why))
    }

    fn recoverable(&self, why: String, how: Box<Recovery>) -> Result<(), Assertion> {
        Err(Assertion::Recoverable(self.name(), why, how))
    }
}

#[derive(Debug)]
pub struct NoCond {
}

impl Condition for NoCond {
    fn check(&self) -> Result<(), Assertion> {
        Ok(())
    }

    fn name(&self) -> String {
        "nop".to_string()
    }
}

/// A condition that always fail as a fatal error. This is useful for middleware
/// to assert misconfigurations of a VM as the condition check will always fail
#[derive(Debug)]
pub struct GenericFatalCondition {
    pub name: String,
    pub message: String
}

impl GenericFatalCondition {
    pub fn new_boxed(name: &str, message: &str) -> Box<dyn Condition> 
    {
        Box::new(GenericFatalCondition {
            name: name.to_string(),
            message: message.to_string()
        })
    }
}

impl Condition for GenericFatalCondition {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        Err(Assertion::Fatal(self.name.to_string(), self.message.to_string()))
    }
}

#[derive(Debug)]
pub struct FatalIoError {
    inner: std::io::Error
}

impl FatalIoError {
    #[allow(dead_code)]
    pub fn boxed_last_os_error(&self) -> Box<dyn Condition>
    {
        Box::new(FatalIoError { inner: std::io::Error::last_os_error() })
    }
}

impl Condition for FatalIoError {
    fn name(&self) -> String {
        "std::io::error".to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        Err(Assertion::Fatal(self.name(), format!("{:#?}", self.inner)))
    }
}

#[derive(Debug)]
pub struct NestedConditions {
    pub name: String,
    pub conditions: Vec<Box<dyn Condition>>
}

impl Condition for NestedConditions {
    fn check(&self) -> Result<(), Assertion> {
        let mut v = vec![];
        for cond in self.conditions.iter() {
            match cond.check() {
                Ok(()) => (),
                Err(condition) => v.push((cond.name(), condition))
            }
        }

        if v.is_empty() { 
            Ok(())
        } else {
            Err(Assertion::Container(v))
        }
    }

    fn name(&self) -> String {
        self.name.to_string()
    }
}


#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum FsEntity {
    /// A regular file is a filesystem
    File(PathBuf),
    /// A directory in a filesystem
    Directory(PathBuf),
    /// A device node in a filesystem
    Node(PathBuf),
    /// Any item "reachable" in a filesystem
    FsItem(PathBuf)
}

impl FsEntity {
    pub fn exists(&self) -> std::result::Result<(), String> {
        macro_rules! iml {
            ($path:expr, $func:ident, $msg:expr) => {
                {
                    let path = std::path::Path::new($path);
                    if path.exists() {
                        if path.$func() {
                            Ok(())
                        } else {
                            Err(format!("Entity {path:?} exists but is not a {}", $msg))
                        }
                    } else {
                        Err(format!("Entity at {path:?} does not exists or do not have permission to access"))
                    }
                }
            }
        }

        match self {
            FsEntity::File(path) => iml!(path, is_file, "regular file"),
            FsEntity::Directory(path) => iml!(path, is_dir, "directory"),
            FsEntity::Node(path) => iml!(path, is_file, "regular file"),
            FsEntity::FsItem(path) => {
                if std::path::Path::new(path).exists() {
                    Ok(())
                } else {
                    Err("Entity does not exists or do not have permission to access".to_string())
                }
            }
        }
    }
}

/// Condition where the `resource` must exists. This is useful to make sure
/// things that the hypervisor required to run (say bootrom) exists.
#[derive(Debug, Clone)]
pub struct Existence {
    pub resource: FsEntity
}

impl Condition for Existence {

    fn name(&self) -> String {
        "exists".to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        self.resource.exists().map_err(|reason| Assertion::Fatal("exists".to_string(), reason))
    }
}

/// Condition where the `resource` must absent. This is useful to make sure
/// the hypervisor does not fail to start because it is not able to create
/// object in the filesystem. An example is the com sockets created by 
/// virtio-console
#[derive(Debug, Clone)]
pub struct Absence {
    pub resource: FsEntity
}

impl Condition for Absence
{
    fn name(&self) -> String {
        "absence".to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        match self.resource.exists() {
            Err(_) => Ok(()),
            Ok(()) => self.assert_failure("Resource alreday exists".to_string())
        }
    }
}

/// The Bhyve hypervisor emulate a number of virutal PCI device for the guest.
/// The PCI selector, the slot and func number of the PCI device BHyve emulated
/// are constrainted to slot# \in [0,31] and bus# \in [0,7]
#[derive(Debug, Clone)]
pub struct ValidBhyveVPciSlot {
    pub slot: PciSlot
}

impl Condition for ValidBhyveVPciSlot {

    fn name(&self) -> String {
        "valid_bhyve_vpci_slot".to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        if self.slot.slot > 31 {
            self.assert_failure("Invalid vPCI slot. Allowed values are between 0 to 31".to_string())
        } else if self.slot.func > 7 {
            self.assert_failure("Invalid vPCI slot. Allowed values are between 0 to 7".to_string())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidPassthruDevice {
    pub slot: PciSlot
}

impl Condition for ValidPassthruDevice {

    fn name(&self) -> String {
        format!("pci0:{}:{}:{}", self.slot.bus, self.slot.slot, self.slot.func)
    }

    fn check(&self) -> Result<(), Assertion> {

        match PciDevice::from_pciconf(&self.slot) {
            None => self.assert_failure("Invalid PCI device".to_string()),
            Some(dev) => {
                if dev.header_type != 0x00 {
                    if dev.header_type == 0x7f {
                        // personal expierence: if pci dev has hdr of 0x7f, it usually
                        // means it's a VF and SR-IOV failed as the motherboard may non
                        // support it
                        self.assert_failure(
                            "This device has invalid HDR of 0x7f, if this is a SR-IOV \
                            VF, please check if the motherboard you are using supports \
                            and enabled SR-IOV".to_string())
                    } else {
                        self.assert_failure(
                            format!(
                                "cannot passthru non-endpoint device, header type: {}",
                                dev.header_type))
                    }
                } else if dev.device_name.starts_with("ppt") {
                    Ok(())
                } else {
                    let slot = Box::new(self.slot);

                    Err(
                        Assertion::Recoverable(self.name(), "pci-attach-ppt".to_string(), Box::new(
                            move || {
                                let mut cc = PciDevice::from_pciconf(&slot).unwrap();
                                cc.force_passthru();
                            }
                        ))
                    )
                }
            }
        }
    }
}

/// Check if networking backend instances are available in the host. Currently
/// support checking tap devices only. Will always reports true for other devices
#[derive(Debug, Clone)]
pub struct NetworkBackendAvailable {
    pub backend: NetBackend,
    pub name: String
}

impl Condition for NetworkBackendAvailable
{
    fn name(&self) -> String {
        "network-backend-available".to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        if let NetBackend::Tap = self.backend {
            let ifaces = get_tap_ifaces().map_err(|s| Assertion::Fatal(self.name(), s.to_string()))?;
            if ifaces.contains(&self.name) {
                if is_tap_opened(&self.name).map_err(|s| Assertion::Fatal(self.name(), s.to_string()))? {
                    self.assert_failure("Tap device exists but is already opened by another process".to_string())
                } else {
                    Ok(())
                }
            } else {

                let name = Box::new(self.name());
                Err(Assertion::Recoverable("tap-iface".to_string(), "create-tap".to_string(),
                    Box::new(move || {
                        cmd!(ifconfig tap create name (name.as_str())).status().unwrap();
                    })))
            }
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct LpcSlotAssignment {
    pub slot: PciSlot
}

impl Condition for LpcSlotAssignment {

    fn name(&self) -> String {
        "lpc_bus".to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        if self.slot.bus != 0 {
            self.assert_failure("Lpc device can only configure on bus 0".to_string())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
pub struct ValidResolution {
    pub h: Option<u32>,
    pub w: Option<u32>
}

impl Condition for ValidResolution {
    fn name(&self) -> String {
        "fbuf_resolution".to_string()
    }

    fn check(&self) -> Result<(), Assertion> {
        if self.h.is_some() && self.w.is_some() {
            let h = self.h.unwrap();
            let w = self.w.unwrap();
            if w > 1920 || h > 1200 {
                self.assert_failure(format!("Maximum resolution is 1920x1200, got {w}x{h}"))
            } else if w < 640 || h < 480 {
                self.assert_failure(format!("Minimum resolution is 640x480, got {w}x{h}"))
            } else {
                Ok(())
            }
        } else if self.h.is_none() && self.w.is_none() {
            Ok(())
        } else {
            self.assert_failure("w and h must either both specified or both unspecified".to_string())
        }
    }
}

#[derive(Debug)]
pub struct KernelFeature {
    kmod: String
}

impl KernelFeature {
    pub fn new_boxed(kmod: &str) -> Box<dyn Condition> {
        Box::new(KernelFeature { kmod: kmod.to_string() })
    }
}

impl Condition for KernelFeature {
    fn name(&self) -> String {
        format!("kmod:{}", self.kmod)
    }

    fn check(&self) -> Result<(), Assertion> {
        match crate::util::os::exists_kld(&self.kmod) {
            Some(true) => Ok(()),
            Some(false) => self.assert_failure(format!("kernel module {} has not loaded", self.kmod)),
            None => self.assert_failure(format!("invalid kmod {}", self.kmod))
        }
    }
}
