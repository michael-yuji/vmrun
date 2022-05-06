# Vmrun

`Vmrun` is a supervisor for single bhyve instance. It is also intend to be a library for anyone to write their own bhyve supervisor in Rust. It is still under heavy development and is planning to support different config format, and change the configuration scheme.

It uses its own configuration scheme (in json; see below) to provide many additional features to bhyve alone. These features include:

- Automatic slot assignment. You can still specify which slot a virtual hardware should be on, otherwise the slot position can be generated.
- Configurable reboot the VM when guest exists (guest initiated reboot, triple fault, etc...)
- Multi-targets can be configurated, each target specifies its difference in configuration from the default target. For example, an installation target with ISO attached.
- Next boot target: a `next_target` field can be specified and will be use for the next boot after prior target. For example, the `next_target` of an install target can be the default target, which boot without the ISO attached.
- Resource management/cleanup: for example, stock Bhyve cannot cleanup socket files left by `virtio-console`; with the supervisor, this can be done easily.

**An example configuration will be:**
In this example, we create a VM with a network adapter backed by interface "tap3", a block device "disk.img", and a com port allow us to use the guest's console .

We also have an extra target "install", which contains an extra "ahci-cd" for mounting installation media. When it halt and reboot, since the "next_target" is the default target, it will boot without "ahci-cd" device.
```json
{
  "name": "freebsd-test",
  "cpu":  2,
  "mem": "512M",
  "com1": "stdio",
  "bootrom": "/usr/local/share/uefi-firmware/BHYVE_UEFI.fd",
  "emulations": [
    {"frontend": "virtio-net", "backend": "tap", "name": "tap3"},
    {"frontend": "virtio-blk", "device": "disk.img"}
  ],
  "targets": {
    "install": {
      "emulations": [
         { "frontend": "ahci-cd"
         , "device": "FreeBSD-13.0-RELEASE-amd64-disc1.iso"
         }
      ],
      "next_target": "default"
    }
  }
}
```
## Relation with `bhyve_config(5)`

This utiltity uses the legacy bhyve config format. This is mostly due to the compactness (despite complex, but it is handled issue) of the old format, there when one run with `-d` or `--dry-run` can inspect a much shorter command; another main reason why the legacy config format is flavoured is to be compatbile with older bhyve release.

## More Documentation coming...
