use crate::vm::{ EmulatedPci, RawEmulatedPci, EmulationOption, Resource
               , Requirement, NetBackend };

#[derive(Debug, Clone)]
pub struct VirtioNet {
    pub tpe:  NetBackend,//VirtioNetBackend,
    pub name: String,
    pub mtu:  Option<u32>,
    pub mac:  Option<String>
}

impl EmulatedPci for VirtioNet
{
    fn preconditions(&self) -> Vec<Requirement> {
        vec![
            Requirement::Exists(
                Resource::Iface(self.tpe, self.name.to_string())
            )
        ]
    }

    fn as_raw(&self) -> RawEmulatedPci {

        let mut options = vec![];

        if let Some(mtu) = self.mtu {
            options.push(
                EmulationOption::KeyValue("mtu".to_string(), mtu.to_string()));
        }

        if let Some(mac) = &self.mac {
            options.push(
                EmulationOption::KeyValue("mac".to_string(), mac.to_string()));
        }

        let tpe = match self.tpe {
            NetBackend::Tap => "tap",
            NetBackend::Netgraph => "netgraph",
            NetBackend::Netmap => "netmap"
        };

        options.push(
            EmulationOption::KeyValue("type".to_string(), tpe.to_string()));

        RawEmulatedPci {
            frontend: "virtio-net".to_string(),
            device:   "virtio-net".to_string(),
            backend:  Some(("backend".to_string(), self.name.to_string())),
            options
        }
    }
}

macro_rules! options_push_on {
    ($options:expr, $self:expr, $value:ident) => {
        if $self.$value {
            $options.push(EmulationOption::On(stringify!($value).to_string()));
        }
    }
}

macro_rules! options_push_kv {
    ($options:expr, $self:expr, $key:ident) => {
        if let Some(value) = &$self.$key {
            $options.push(EmulationOption::KeyValue(
                    stringify!($key).to_string(), value.to_string()));
        }
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
    pub nodelete: bool
}

impl EmulatedPci for VirtioBlk
{
    fn preconditions(&self) -> Vec<Requirement> {
        vec![Requirement::Exists(Resource::FsItem(self.path.to_string()))]
    }

    fn as_raw(&self) -> RawEmulatedPci {
        let mut options = vec![];
        options_push_on!(options, self, direct);
        options_push_on!(options, self, nocache);
        options_push_on!(options, self, ro);
        options_push_on!(options, self, nodelete);

        if let Some(logical) = self.logical_sector_size {
            let value = match self.physical_sector_size {
                    Some(physical) => format!("{}/{}",logical,physical),
                    None => format!("{}", logical)
                };
            options.push(EmulationOption::KeyValue(
                    "sectorsize".to_string(), value));
        }

        RawEmulatedPci {
            frontend: "virtio-blk".to_string(),
            device:   "virtio-blk".to_string(),
            backend:  Some(("path".to_string(), self.path.to_string())),
            options
        }
    }
}

macro_rules! mk_ahci_frontend {
    ($name:ident, $value:literal) => {
        #[derive(Debug, Clone)]
        pub struct $name {
            pub path: String,
            pub nmrr:   Option<u32>,
            pub ser:    Option<String>,
            pub rev:    Option<String>,
            pub model:  Option<String>
        }

        impl EmulatedPci for $name {

            fn preconditions(&self) -> Vec<Requirement> {
                vec![Requirement::Exists(
                    Resource::FsItem(self.path.to_string()))]
            }

            fn as_raw(&self) -> RawEmulatedPci {
                let mut options = vec![];

                options_push_kv!(options, self, nmrr);
                options_push_kv!(options, self, ser);
                options_push_kv!(options, self, rev);
                options_push_kv!(options, self, model);

                RawEmulatedPci {
                    frontend: $value.to_string(),
                    device:   "ahci".to_string(),
                    backend:  Some(("path".to_string(), self.path.to_string())),
                    options
                }
            }
        }
    }
}
mk_ahci_frontend!(AhciCd, "ahci-cd");
mk_ahci_frontend!(AhciHd, "ahci-hd");

#[derive(Debug, Clone)]
pub struct VirtioConsole {
    pub ports: Vec<String>
}

impl EmulatedPci for VirtioConsole {

    fn preconditions(&self) -> Vec<Requirement> {
        self.ports.iter()
            .map(|port| Requirement::Nonexists(
                    Resource::FsItem(port.to_string()))
                )
            .collect()
    }

    fn ephemeral_objects(&self) -> Vec<Resource> {
        self.ports.iter().map(|port| Resource::Node(port.to_string())).collect()
    }

    fn as_raw(&self) -> RawEmulatedPci {
        let mut options = vec![];
        for index in 0..self.ports.len() {
            let opt = EmulationOption
                ::KeyValue(format!("port{}", index + 1), self.ports[index].to_string());
            options.push(opt);
        }

        RawEmulatedPci {
            frontend: "virtio-console".to_string(),
            device:   "virtio-console".to_string(),
            backend:  None,
            options
        }
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
    pub password: Option<String>
}

impl EmulatedPci for Framebuffer {
    fn as_raw(&self) -> RawEmulatedPci {
        let mut options = vec![];
        options_push_on!(options, self, wait);
        options_push_kv!(options, self, h);
        options_push_kv!(options, self, w);
        options_push_kv!(options, self, password);
        options_push_kv!(options, self, vga);

        let rfb = if let Some(port) = &self.port {
            format!("{}:{}", self.host, port)
        } else {
            format!("{}", self.host)
        };

        options.push(EmulationOption::KeyValue("rfb".to_string(), rfb));

        RawEmulatedPci {
            frontend: "fbuf".to_string(),
            device:   "fbuf".to_string(),
            backend: None,
            options
        }
    }
}

#[derive(Debug, Clone)]
pub struct Xhci {
}

impl EmulatedPci for Xhci {
    fn as_raw(&self) -> RawEmulatedPci {
        RawEmulatedPci {
            frontend: "xhci".to_string(),
            device:   "xhci".to_string(),
            backend:  Some(("slot.1.device".to_string(), "tablet".to_string())),
            options:  vec![]
        }
    }
}

