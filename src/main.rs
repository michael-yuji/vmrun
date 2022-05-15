
mod vm;
mod spec;
mod util;

use clap::Parser;
use vm::{VmError};
use spec::FormatError;
use std::io::{Read, Write};
use std::process;
use thiserror::Error;

#[derive(Error, Debug)]
enum VmRunError {
    #[error("vmerror::{0}")]
    VmErr(VmError),
    #[error("format_error::{0}")]
    SpecErr(FormatError),
    #[error("One of more precondition failed: {0}")]
    PreconditionFailure(String),
    #[error("{0}")]
    IoError(std::io::Error)
}

/* To work around clap */
#[derive(Debug)]
struct ArgVec<T> {
    vec: Vec<T>
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
    config:  String,

    /// Do not follow reboots initiated by the guest
    #[clap(long)]
    no_reboot: bool,

    /// Print the bhyve command to stdout and exit
    #[clap(short = 'D', long)]
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
    panic_on_failed_cleaup: bool,

    /// Do not check resources requirement prior to launch bhyve
    #[clap(long)]
    no_requirement_check: bool,

    /// Dump the resultant configuration and exit
    #[clap(long)]
    debug: bool,

    /// arguments that get passed directly to bhyve
    #[clap(raw = true, value_name = "BHYVE_ARGS")]
    extra_bhyve_args: Vec<String>
}

fn arg_to_vec(s: &str) -> Result<ArgVec<i32>, &'static str> {
    let mut parts = s.split(",");
    let mut vec = Vec::<i32>::new();
    while let Some(part) = parts.next() {
        let int = part.parse::<i32>()
            .map_err(|_e| "invalid value encountered while parsing i32 list")?;
        vec.push(int);
    }
    Ok(ArgVec { vec })
}
fn vm_main(args: &Arguments, vm: &spec::VmSpec) -> Result<i32, VmRunError>
{
    let mut spec = vm.clone();
    let mut reboot_count = 0;
    let mut next_target = args.target.clone();
    let mut exit_code: i32;

    fn vm_run_session(args: &Arguments, vmrun: &vm::VmRun) -> Result<i32, VmRunError>
    {
       let bootargs = vmrun.bhyve_args().map_err(|e| VmRunError::VmErr(e))?;
       let hyve = std::option_env!("BHYVE_EXEC").unwrap_or("bhyve");

       if args.debug {
           println!("{:#?}", vmrun);
       }

        if args.dry_run || args.debug {
            print!("{} ", hyve);
            for arg in bootargs {
                print!("{} ", arg);
            }
            println!();

            let cfs = vmrun.bhyve_conf_opts()
                .map_err(|e| VmRunError::VmErr(e))?;
            for cf in cfs.iter() {
                println!("-o {}", cf);
            }

            return Ok(0);
        }

        let pid_file = match &args.vm_pid_file {
            Some(pid_file) => Some(open_pid_file(pid_file)?),
            None => None
        };

        let mut process = std::process::Command::new(hyve)
            .args(&bootargs).spawn().ok().unwrap();

        if let Some(mut pid_file) = pid_file {
            pid_file.write(process.id().to_string().as_bytes())
                .map_err(|e| VmRunError::IoError(e))?;
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
                spec.consume_target(target).map_err(|e| VmRunError::SpecErr(e))?;
            }
        }

        next_target = spec.next_target.clone();

        let vmrun = spec.build(&args.extra_bhyve_args)
            .map_err(|e| VmRunError::SpecErr(e))?;

        if !args.no_requirement_check {
            for requirement in vmrun.preconditions() {
                if !requirement.is_satisfied() {
                    return Err(VmRunError::PreconditionFailure(
                            requirement.warning()));
                }
            }
        }

        let run_result = vm_run_session(&args, &vmrun);
        
        for object in vmrun.ephemeral_objects() {
            match object.release() {
                Err(error) => {
                    if args.panic_on_failed_cleaup {
                        panic!("Error occured when cleaning up: {}", error);
                    } else {
                        eprintln!("warn: Error occured when cleaning up: {}", error);
                    }
                },
                Ok(_) => continue
            }
        }

        exit_code = if let Ok(ec) = run_result { ec } else { 4 };

        /* if exit code is 0, it means the guest wanna reboot */
        if reboot_count < args.reboot_count.unwrap_or(usize::MAX)
            && args.reboot_on.contains(&exit_code) && run_result.is_ok()
            && !args.dry_run && !args.no_reboot
        {
            reboot_count += 1;
            continue
        } else {
            _ = run_result?;
            break
        }
    };
    
    Ok(exit_code)
}

fn open_pid_file<P: AsRef<std::path::Path>>(path: P) 
  -> Result<std::fs::File, VmRunError>
{
    if let Ok(metadata) = std::fs::metadata(path.as_ref()) {
        if !metadata.is_file() {
            Err(VmRunError::PreconditionFailure(
                format!("{:?} is not a regular file", path.as_ref())))
        } else if metadata.permissions().readonly() {
            Err(VmRunError::PreconditionFailure(
                format!("{:?} is not writable", path.as_ref())))
        } else {
            std::fs::File::options().write(true).open(path)
                .map_err(|e| VmRunError::IoError(e))
        }
    } else {
        if let Some(_parent) = path.as_ref().parent() {
            std::fs::File::create(path).map_err(|e| VmRunError::IoError(e))
        } else {
            Err(VmRunError::PreconditionFailure(
                format!("Parent to {:?} is not available", path.as_ref())))
        }
    }
}

fn write_pid_file<P: AsRef<std::path::Path>>(path: P, pid: u32) 
    -> Result<(), VmRunError> 
{
    let mut file = open_pid_file(path)?;
    file.write(pid.to_string().as_bytes()).map_err(|e| VmRunError::IoError(e))?;
    Ok(())
}

fn main() {
    let args = Arguments::parse();
    let mut content: String = String::new();

    content = 
        if args.config.as_str() == "-" {
            let mut stdin = std::io::stdin();
            stdin.read_to_string(&mut content).expect("Error reading stdin");
            content
        } else {
            std::fs::read_to_string(args.config.to_string())
               .expect("fail to read configuration file")
        };

    if let Some(file) = &args.supervisor_pid_file {
        if let Err(err) = write_pid_file(file, process::id()) {
            eprintln!("cannot write supervisor pid file: {}", err);
            process::exit(4);
        }
    }

    let vm: spec::VmSpec = serde_json::from_str(&content).expect("malformed config");
    match vm_main(&args, &vm) {
        Err(error) => println!("vmrun exited with error: {}", error),
        Ok(exit_code) => std::process::exit(exit_code)
    }
}
