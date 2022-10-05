use crate::vm::conditions::{
    Absence, Condition, Existence, FsEntity, NestedConditions, NetworkBackendAvailable, NoCond,
    ValidPassthruDevice, ValidResolution,
};
use crate::vm::{BhyveArg, BhyveDev, EmulatedPci, NetBackend, PciSlot, Resource};

macro_rules! push_on_options {
    ($base:expr, $self:expr, $value:ident) => {
        if $self.$value {
            $base.push_str(format!(",{}", stringify!($value)).as_str());
        }
    };
}

macro_rules! push_on_kv {
    ($base:expr, $self:expr, $key:ident) => {
        if let Some(value) = &$self.$key {
            $base.push_str(format!(",{}={}", stringify!($key), value).as_str());
        }
    };
}

macro_rules! push_on_key_value {
    ($arr:expr, $self:expr, $key:ident) => {
        if let Some(value) = &$self.$key {
            $arr.push((stringify!($key).to_string(), format!("{}", value)));
        }
    };
}

#[derive(Debug, Clone)]
pub struct VirtioNet {
    pub tpe: NetBackend,
    pub name: String,
    pub mtu: Option<u32>,
    pub mac: Option<String>,
}

impl BhyveDev for VirtioNet {
    fn preconditions(&self) -> Box<dyn Condition> {
        Box::new(NetworkBackendAvailable {
            backend: self.tpe,
            name: self.name.to_string(),
        })
    }
}

impl EmulatedPci for VirtioNet {
    fn as_bhyve_arg(&self) -> BhyveArg {
        let mut base = format!("virtio-net,{},type={}", self.name, self.tpe.to_string());
        push_on_kv!(base, self, mtu);
        push_on_kv!(base, self, mac);
        BhyveArg::Legacy(base)
    }
}

#[derive(Debug, Clone)]
pub struct VirtioBlk {
    pub path: String,
    pub nocache: bool,
    pub direct: bool,
    pub ro: bool,
    pub logical_sector_size: Option<u32>,
    pub physical_sector_size: Option<u32>,
    pub nodelete: bool,
}

impl BhyveDev for VirtioBlk {
    fn preconditions(&self) -> Box<dyn Condition> {
        let path = std::path::PathBuf::from(self.path.to_string());
        Box::new(Existence {
            resource: FsEntity::File(path),
        })
    }
}

impl EmulatedPci for VirtioBlk {
    fn as_bhyve_arg(&self) -> BhyveArg {
        let mut base = format!("virtio-blk,{}", self.path);
        push_on_options!(base, self, direct);
        push_on_options!(base, self, nocache);
        push_on_options!(base, self, ro);
        push_on_options!(base, self, nodelete);

        if let Some(logical) = self.logical_sector_size {
            let value = match self.physical_sector_size {
                Some(physical) => format!("{logical}/{physical}"),
                None => format!("{logical}"),
            };

            base.push_str(format!("sectorsize={value}").as_str());
        }

        BhyveArg::Legacy(base)
    }
}

macro_rules! mk_ahci_frontend {
    ($name:ident, $value:literal) => {
        #[derive(Debug, Clone)]
        pub struct $name {
            pub path: String,
            pub nmrr: Option<u32>,
            pub ser: Option<String>,
            pub rev: Option<String>,
            pub model: Option<String>,
        }

        impl EmulatedPci for $name {
            fn as_bhyve_arg(&self) -> BhyveArg {
                let mut base = format!("{},{}", $value, self.path);
                push_on_kv!(base, self, nmrr);
                push_on_kv!(base, self, ser);
                push_on_kv!(base, self, rev);
                push_on_kv!(base, self, model);
                BhyveArg::Legacy(base)
            }
        }

        impl BhyveDev for $name {
            fn preconditions(&self) -> Box<dyn Condition> {
                let pathbuf = std::path::PathBuf::from(self.path.to_string());
                Box::new(Existence {
                    resource: FsEntity::FsItem(pathbuf),
                })
            }
        }
    };
}
mk_ahci_frontend!(AhciCd, "ahci-cd");
mk_ahci_frontend!(AhciHd, "ahci-hd");

#[derive(Debug, Clone)]
pub enum NvmeBackend {
    Ram(usize),
    Path(String),
}

#[derive(Debug, Clone)]
pub struct Nvme {
    pub qsz: Option<u32>,
    pub ioslots: Option<u32>,
    pub sectsz: Option<u32>,
    pub ser: Option<String>,
    pub eui64: Option<u32>,
    pub dsm: Option<String>,
    pub backend: NvmeBackend,
}

impl BhyveDev for Nvme {
    fn preconditions(&self) -> Box<dyn Condition> {
        if let NvmeBackend::Path(path) = &self.backend {
            Box::new(Existence {
                resource: FsEntity::FsItem(std::path::PathBuf::from(path)),
            })
        } else {
            Box::new(NoCond {})
        }
    }
}

impl Nvme {
    pub fn from_ram_mb(mbs: usize) -> Nvme {
        Nvme {
            backend: NvmeBackend::Ram(mbs),
            qsz: None,
            ioslots: None,
            sectsz: None,
            ser: None,
            eui64: None,
            dsm: None,
        }
    }

    pub fn from_file(path: String) -> Nvme {
        Nvme {
            backend: NvmeBackend::Path(path),
            qsz: None,
            ioslots: None,
            sectsz: None,
            ser: None,
            eui64: None,
            dsm: None,
        }
    }
}

impl EmulatedPci for Nvme {
    fn as_bhyve_arg(&self) -> BhyveArg {
        let mut config = vec![("device".to_string(), "nvme".to_string())];
        push_on_key_value!(config, self, qsz);
        push_on_key_value!(config, self, ioslots);
        push_on_key_value!(config, self, sectsz);
        push_on_key_value!(config, self, ser);
        push_on_key_value!(config, self, eui64);
        push_on_key_value!(config, self, dsm);
        match &self.backend {
            NvmeBackend::Ram(size) => config.push(("ram".to_string(), size.to_string())),
            NvmeBackend::Path(file) => config.push(("path".to_string(), file.to_string())),
        };
        BhyveArg::Config(config)
    }
}

#[derive(Debug, Clone)]
pub struct VirtioConsole {
    pub ports: Vec<String>,
}

impl BhyveDev for VirtioConsole {
    fn preconditions(&self) -> Box<dyn Condition> {
        let mut cond: Vec<Box<dyn Condition>> = vec![];

        for port in self.ports.iter() {
            let port = std::path::PathBuf::from(port);
            cond.push(Box::new(Absence {
                resource: FsEntity::FsItem(port),
            }))
        }

        Box::new(NestedConditions {
            name: "virtio-console".to_string(),
            conditions: cond,
        })
    }
}

impl EmulatedPci for VirtioConsole {
    fn ephemeral_objects(&self) -> Vec<Resource> {
        self.ports
            .iter()
            .map(|port| Resource::Node(port.to_string()))
            .collect()
    }

    fn as_bhyve_arg(&self) -> BhyveArg {
        let mut base = "virtio-console".to_string();
        for index in 0..self.ports.len() {
            base.push_str(format!(",port{}={}", index + 1, self.ports[index]).as_str());
        }
        BhyveArg::Legacy(base)
    }
}

#[derive(Debug, Clone)]
pub struct PciPassthru {
    pub src: PciSlot,
    pub rom: Option<String>,
}

impl EmulatedPci for PciPassthru {
    fn as_bhyve_arg(&self) -> BhyveArg {
        let mut base = format!("passthru,{}", self.src.as_passthru_arg());
        push_on_kv!(base, self, rom);
        BhyveArg::Legacy(base)
    }
}

impl BhyveDev for PciPassthru {
    fn preconditions(&self) -> Box<dyn Condition> {
        println!("pci passthru preconditions");
        let mut base: Vec<Box<dyn Condition>> =
            vec![Box::new(ValidPassthruDevice { slot: self.src })];

        match &self.rom {
            None => (),
            Some(rom) => {
                let rom = std::path::PathBuf::from(rom);
                base.push(Box::new(Existence {
                    resource: FsEntity::File(rom),
                }))
            }
        };

        Box::new(NestedConditions {
            name: "passthru".to_string(),
            conditions: base,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Framebuffer {
    pub host: String,
    pub port: Option<u16>,
    pub w: Option<u32>,
    pub h: Option<u32>,
    pub vga: Option<String>,
    pub wait: bool,
    pub password: Option<String>,
}

impl BhyveDev for Framebuffer {
    fn preconditions(&self) -> Box<dyn Condition> {
        Box::new(ValidResolution {
            w: self.w,
            h: self.h,
        })
    }
}

impl EmulatedPci for Framebuffer {
    fn as_bhyve_arg(&self) -> BhyveArg {
        let mut base = format!("fbuf,tcp={}:{}", self.host, self.port.unwrap_or(5900));
        push_on_kv!(base, self, w);
        push_on_kv!(base, self, h);
        push_on_kv!(base, self, vga);
        push_on_kv!(base, self, password);
        push_on_options!(base, self, wait);
        BhyveArg::Legacy(base)
    }
}

#[derive(Debug, Clone)]
pub struct Xhci {}

impl BhyveDev for Xhci {
    fn preconditions(&self) -> Box<dyn Condition> {
        Box::new(NoCond {})
    }
}

impl EmulatedPci for Xhci {
    fn as_bhyve_arg(&self) -> BhyveArg {
        BhyveArg::Legacy("xhci,tablet".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_passthru_format() {
        let slot = PciSlot {
            bus: 1,
            slot: 1,
            func: 1,
        };
        let device = PciPassthru {
            src: slot,
            rom: None,
        };
        let device2 = PciPassthru {
            rom: Some("1.fd".to_string()),
            ..device
        };
        assert_eq!(
            device.as_bhyve_arg(),
            BhyveArg::Legacy("passthru,1/1/1".to_string())
        );
        assert_eq!(
            device2.as_bhyve_arg(),
            BhyveArg::Legacy("passthru,1/1/1,rom=1.fd".to_string())
        );
    }

    #[test]
    fn xhci_format() {
        let device = Xhci {};
        assert_eq!(
            device.as_bhyve_arg(),
            BhyveArg::Legacy("xhci,tablet".to_string())
        );
    }

    #[test]
    fn framebuffer_format() {
        let fb = Framebuffer {
            host: "0.0.0.0".to_string(),
            port: None,
            w: Some(1280),
            h: Some(920),
            vga: None,
            wait: true,
            password: None,
        };

        assert_eq!(
            fb.as_bhyve_arg(),
            BhyveArg::Legacy("fbuf,host=0.0.0.0,w=1280,h=920,wait".to_string())
        );
    }
}
