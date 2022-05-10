# Vmrun

`Vmrun` is a supervisor for single bhyve instance. It is intended to provide a developer friendly layer to the default FreeBSD bhyve userland. This utility accepts `JSON` configuration from either filesystem or `stdin`.

The goal is to make bhyve
- easier to test different configurations with `next_target` and reboot conditions.
- easier to integrate with other software.
- easier to use and reduce accidential side effects. 

*Another goal is to make bhyve more programmable and make it easier to write vm manager for FreeBSD Bhyve userland by providing a crate.*


## Features

- Automatic slot assignment 
  - Only need to specify devices, this tool can figure out the slot assignment.
  - You can still specify which slot should the devices on.
- Multi-targets
  - Targets are additional configurations that replace/merge with the main configuration when selected.
  - For example a `install` target can have an extra installation media device attached. 
- Configurable reboot / next boot target
  - Configure if the bhyve instance should reboot on different exit codes
  - The `next_target` field define a target to be used when the current target reboots.
- House keeping
  - Automatically cleanup ephemeral resources left by bhyve. For example `*.sock` left by `virtio-console`
 
## Configuration
**An example configuration will be:**
In this example, we create a VM with a network adapter backed by interface "tap3", a block device "disk.img", and a com port allow us to use the guest's console .

We also have an extra target "install", which contains an extra "ahci-cd" for mounting installation media. When it halt and reboot, since the "next_target" is the default target, it will boot without the "ahci-cd" device.
```json
{
  "name": "freebsd-test",
  "cpu":  2,
  "mem": "512M",
  "com1": "stdio",
  "bootrom": "/usr/local/share/uefi-firmware/BHYVE_UEFI.fd",
  "emulations": [
    {"device": "virtio-net", "name": "tap3"},
    {"device": "virtio-blk", "path": "disk.img"}
  ],
  "targets": {
    "install": {
      "emulations": [
         { "device": "ahci-cd"
         , "path": "FreeBSD-13.0-RELEASE-amd64-disc1.iso"
         }
      ],
      "next_target": "default"
    }
  }
}
```

Since the project is very young and the scheme of the configuration will likely change quite a bit and nowhere close to stable yet. The best reference is currently the source code (`src/spec/mod.rs`), hopefully the situation will be better soon. 

### Configure with UCL config files
You can use UCL for configuration by piping the Json representation of the configuration file to the `stdin` (by using `-c-`) of this utility.

For example: `uclcmd get -c -f myvm.conf | vmrun -c-`

UCL is the configuration format most used in FreeBSD. Native UCL is currently not supported.

*Unfortunately there is currently no plan to support UCL (at least not fully). This is 
mostly because UCL itself is not currently a very consistent config format (try some edge cases yourself), and can introduce a lot of side effects. This go against the goal of this utility to be predictable on itself.*


## Debugging issues
An option `-d` or `--dry-run` is available to print out the equalivent bhyve command that will be executed. Error handling and error messages are currently lacking because this project is still very new (by the time of writing it is <1week old)

The `--debug` option can also be used to print the parsed configuration.

## Relation with `bhyve_config(5)`

This utiltity uses the legacy bhyve config format instead of the new bhyve config file and config syntax. Not only this allows the utility to work with eariler verson of bhyve, it also makes debugging slightly easier due to the syntax being more compact and better documented.


This is mostly due to the compactness (deof the old format, there when one run with `-d` or `--dry-run` can inspect a much shorter command; another main reason why the legacy config format is flavoured is to be compatbile with older bhyve release.

## More Documentation coming...

