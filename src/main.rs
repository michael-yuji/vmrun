mod spec;
mod util;
mod vm;

use clap::Parser;
use spec::FormatError;
use std::io::{Read, Write};
use std::process;
use thiserror::Error;
use util::assertion::Assertion;
use vm::BhyveDev;

#[derive(Error, Debug)]
enum VmRunError {
    #[error("vmerror::{0}")]
    VmErr(Assertion),
    #[error("format_error::{0}")]
    SpecErr(FormatError),
    #[error("One of more precondition failed: {0}")]
    PreconditionFailure(String),
    #[error("{0}")]
    IoError(std::io::Error),
}

/* To work around clap */
#[derive(Debug)]
struct ArgVec<T> {
    vec: Vec<T>,
}

impl<T: PartialEq> ArgVec<T> {
    fn contains(&self, val: &T) -> bool {
        self.vec.contains(val)
    }
}

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Arguments {
    /// Boot wth a target (boot option) specified in the config file
    #[clap(short, long)]
    target: Option<String>,

    /// The location of the Json configuration file. If `config` is `-`, the
    /// configuratino will read from the stdin stream instead.
    #[clap(short, long, value_name = "FILE")]
    config: String,

    /// Do not follow reboots initiated by the guest
    #[clap(long)]
    no_reboot: bool,

    #[clap(long, short)]
    force: bool,

    #[clap(long)]
    recover: bool,

    /// Print the bhyve command to stdout and exit
    #[clap(long)]
    dry_run: bool,

    /// Maximum number of reboots allowed, default unlimited
    #[clap(long)]
    reboot_count: Option<usize>,

    /// Reboot if bhyve exit with the codes, separate by ",". for example 0,1
    #[clap(long, parse(try_from_str = arg_to_vec), default_value="0")]
    reboot_on: ArgVec<i32>,

    /// Write pid of the bhyve process to the specified location
    #[clap(short = 'p')]
    vm_pid_file: Option<String>,

    /// Write pid of the supervisor process to the specified location
    #[clap(short = 'P')]
    supervisor_pid_file: Option<String>,

    /// Do not proceed if any cleaup failed
    #[clap(long)]
    panic_on_failed_cleanup: bool,

    /// Do not check resources requirement prior to launch bhyve
    #[clap(long)]
    no_requirement_check: bool,

    /// Dump the resultant configuration and exit
    #[clap(long)]
    debug: bool,

    /// arguments that get passed directly to bhyve
    #[clap(raw = true, value_name = "BHYVE_ARGS")]
    extra_bhyve_args: Vec<String>,
}

fn arg_to_vec(s: &str) -> Result<ArgVec<i32>, &'static str> {
    let parts = s.split(',');
    let mut vec = Vec::<i32>::new();
    for part in parts {
        let int = part
            .parse::<i32>()
            .map_err(|_e| "invalid value encountered while parsing i32 list")?;
        vec.push(int);
    }
    Ok(ArgVec { vec })
}

fn ask_yesno(question: String) -> bool {
    println!("{question}? [y/N] (default: No)");
    let mut buffer = String::new();
    let stdin = std::io::stdin();
    stdin.read_line(&mut buffer).unwrap();
    buffer.starts_with('Y') || buffer.starts_with('y')
}

fn vm_main(args: &Arguments, vm: &spec::VmSpec) -> Result<i32, VmRunError> {
    let mut spec = vm.clone();
    let mut reboot_count = 0;
    let mut next_target = args.target.clone();
    let mut exit_code: i32;

    fn vm_run_session(
        args: &Arguments,
        spec: &spec::VmSpec,
        vmrun: &vm::VmRun,
    ) -> Result<i32, VmRunError> {
        let bootargs = vmrun.bhyve_args().map_err(VmRunError::VmErr)?;
        let hyve = std::option_env!("BHYVE_EXEC").unwrap_or("bhyve");

        if args.debug {
            eprintln!("{:#?}", vmrun);
        }

        if args.dry_run || args.debug {
            eprint!("{} ", hyve);
            for arg in bootargs {
                eprint!("{} ", arg);
            }
            eprintln!();
            return Ok(0);
        }

        let pid_file = match &args.vm_pid_file {
            Some(pid_file) => Some(open_pid_file(pid_file)?),
            None => None,
        };

        let dev = std::path::PathBuf::from(format!("/dev/vmm/{}", vmrun.name));
        if dev.exists() && args.force {
            std::process::Command::new("bhyvectl")
                .arg("--destroy")
                .arg(format!("--vm={}", vmrun.name))
                .spawn()
                .ok()
                .unwrap();
        }

        let mut process = std::process::Command::new(hyve)
            .args(&bootargs)
            .spawn()
            .ok()
            .unwrap();

        if let Some(mut pid_file) = pid_file {
            pid_file
                .write(process.id().to_string().as_bytes())
                .map_err(VmRunError::IoError)?;
        }

        if let Some(action) = &spec.post_start_script {
            let args: Vec<&str> = action.split(' ').collect();
            let mut p = std::process::Command::new(args[0])
                .args(&args[1..])
                .spawn()
                .ok()
                .unwrap();
            p.wait().ok().unwrap();
        }

        let exit_status = process.wait().ok().unwrap();
        Ok(exit_status.code().unwrap())
    }

    loop {
        /* if the current target specified next target to run */
        if let Some(target) = &next_target {
            /*
             * The root config itself is the default target unless another
             * target named "default" explicitly defined
             */
            if target == "default" && !spec.has_target(target) {
                spec = vm.clone();
            } else {
                spec.consume_target(target).map_err(VmRunError::SpecErr)?;
            }
        }

        next_target = spec.next_target.clone();

        let vmrun = spec
            .build(&args.extra_bhyve_args)
            .map_err(VmRunError::SpecErr)?;

        // Check if every requirements are archieved before handing to bhyve
        if !args.no_requirement_check {
            // if the user put "fix": true, we apply the known fix to the device
            for emulation in vmrun.emulations.iter() {
                let cond = emulation.preconditions();
                if let Err(assertion) = cond.check() {
                    if assertion.is_recoverable()
                        && (emulation.want_fix
                            || args.force
                            || ask_yesno(assertion.recovery_prompt()))
                    {
                        assertion.recover();
                    }
                }
            }

            let condition = vmrun.preconditions();
            match condition.check() {
                Ok(()) => (),
                Err(assertion) => {
                    println!("{}", assertion.print("vm".to_string()));
                    if !assertion.is_recoverable() {
                        return Err(VmRunError::PreconditionFailure(
                            "One of more fatal failure encountered".to_string(),
                        ));
                    }
                }
            }
        }

        let run_result = vm_run_session(args, vm, &vmrun);

        if args.debug || args.dry_run {
            return Ok(0);
        }

        for object in vmrun.ephemeral_objects() {
            match object.release() {
                Err(error) => {
                    if args.panic_on_failed_cleanup {
                        panic!("Error occured when cleaning up: {}", error);
                    } else {
                        eprintln!("warn: Error occured when cleaning up: {}", error);
                    }
                }
                Ok(_) => continue,
            }
        }

        exit_code = if let Ok(ec) = run_result { ec } else { 4 };

        /* if exit code is 0, it means the guest wanna reboot */
        if reboot_count < args.reboot_count.unwrap_or(usize::MAX)
            && args.reboot_on.contains(&exit_code)
            && run_result.is_ok()
            && !args.dry_run
            && !args.no_reboot
        {
            reboot_count += 1;
            continue;
        } else {
            _ = run_result?;
            break;
        }
    }

    Ok(exit_code)
}

fn open_pid_file<P: AsRef<std::path::Path>>(path: P) -> Result<std::fs::File, VmRunError> {
    if let Ok(metadata) = std::fs::metadata(path.as_ref()) {
        if !metadata.is_file() {
            Err(VmRunError::PreconditionFailure(format!(
                "{:?} is not a regular file",
                path.as_ref()
            )))
        } else if metadata.permissions().readonly() {
            Err(VmRunError::PreconditionFailure(format!(
                "{:?} is not writable",
                path.as_ref()
            )))
        } else {
            std::fs::File::options()
                .write(true)
                .open(path)
                .map_err(VmRunError::IoError)
        }
    } else if let Some(_parent) = path.as_ref().parent() {
        std::fs::File::create(path).map_err(VmRunError::IoError)
    } else {
        Err(VmRunError::PreconditionFailure(format!(
            "Parent to {:?} is not available",
            path.as_ref()
        )))
    }
}

fn write_pid_file<P: AsRef<std::path::Path>>(path: P, pid: u32) -> Result<(), VmRunError> {
    let mut file = open_pid_file(path)?;
    file.write(pid.to_string().as_bytes())
        .map_err(VmRunError::IoError)?;
    Ok(())
}

fn main() {
    let args = Arguments::parse();
    let mut content: String = String::new();

    content = if args.config.as_str() == "-" {
        let mut stdin = std::io::stdin();
        stdin
            .read_to_string(&mut content)
            .expect("Error reading stdin");
        content
    } else {
        std::fs::read_to_string(&args.config).expect("fail to read configuration file")
    };

    if let Some(file) = &args.supervisor_pid_file {
        if let Err(err) = write_pid_file(file, process::id()) {
            eprintln!("cannot write supervisor pid file: {}", err);
            process::exit(4);
        }
    }

    let vm_err: Result<spec::VmSpec, _> = serde_json::from_str(&content)
        .map_err(|err| format_serde_error::SerdeError::new(content.to_string(), err));

    if let Err(e) = vm_err {
        eprintln!("{e}");
        process::exit(4);
    }

    let vm = vm_err.unwrap();

    match vm_main(&args, &vm) {
        Err(error) => println!("vmrun exited with error: {}", error),
        Ok(exit_code) => std::process::exit(exit_code),
    }
}
