
mod vm;
mod spec;
mod util;

use clap::Parser;
use vm::{VmError, Requirement, Resource, CleanupSemantic};
use spec::FormatError;
use thiserror::Error;

#[derive(Error, Debug)]
enum VmRunError {
	#[error("vmerror::{0}")]
	VmErr(VmError),
	#[error("format_error::{0}")]
	SErr(FormatError),
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

    /// The JSON file with the configuration
    #[clap(short, long, value_name = "FILE")]
    config:  String,

    /// Do not follow reboots initiated by the guest
    #[clap(short, long)]
    no_reboot: bool,

    /// Print the bhyve command to stdout and exit
    #[clap(short, long)]
    dry_run: bool,

    /// Maximum number of reboots allowed, default unlimited
    #[clap(long)]
    reboot_count: Option<usize>,

    /// Reboot if bhyve exit with the codes, separate by ",". for example 0,1
    #[clap(long, parse(try_from_str = arg_to_vec), default_value="0")]
    reboot_on: ArgVec<i32>,

    /// Do not proceed to other cleanup if any cleaup failed
    #[clap(long)]
    panic_on_failed_cleaup: bool
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

fn vm_main(args: &Arguments, vm: &spec::VmSpec) -> Result<(), VmRunError>
{
    let mut spec = vm.clone();
    let mut reboot_count = 0;
    let mut next_target = args.target.clone();

    let mut created_resource: Vec<Resource> = Vec::new();

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
	            spec.consume_target(target).map_err(|e| VmRunError::SErr(e))?;
	        }
        }

        next_target = spec.next_target.clone();

        let vmrun = spec.build().map_err(|e| VmRunError::SErr(e))?;


        for requirement in vmrun.requirements() {

            if !requirement.is_satisfied() {
                println!("warning: {}", requirement.warning());
            }

            /* 
             * Bhyve sometimes may create resource, and if resource does not 
             * exists prior to the creation of the VM and left over in the 
             * system, we may want to clean it up
             */
            if let Requirement::MayCreate(
                resource, CleanupSemantic::Borrow) = requirement {
                if !(resource.exists() || created_resource.contains(&resource)) {
                    created_resource.push(resource.clone());
                }
            }
        }

        let bootargs = vmrun.bhyve_args().map_err(|e| VmRunError::VmErr(e))?;
        let hyve = std::option_env!("BHYVE_EXEC").unwrap_or("bhyve");

        if args.dry_run {
            print!("{} ", hyve);
            for arg in bootargs {
                print!("{} ", arg);
            }
            println!();
            return Ok(());
        }

        let mut process = std::process::Command::new(hyve).args(&bootargs).spawn().ok().unwrap();
        let exit_status = process.wait().ok().unwrap();
        let exit_code = exit_status.code().unwrap();

        /* if exit code is 0, it means the guest wanna reboot */
        if reboot_count < args.reboot_count.unwrap_or(usize::MAX)
            && args.reboot_on.contains(&exit_code) && !args.no_reboot
        {
            reboot_count += 1;
            continue
        } else {
            break
        }
    };
    
    for res in created_resource.iter() {
        match res.release() {
            Err(error) => {
                if args.panic_on_failed_cleaup {
                    panic!("Error occured while cleaning up: {}", error);
                } else {
                    println!("warn: Error occured while cleaning up: {}", error);

                }
            },
            Ok(_) => continue 
        }
    }

    Ok(())
}

fn main() {
    let args = Arguments::parse();
    let content = std::fs::read_to_string(args.config.to_string()).expect("fail to read configuration file");
    let vm: spec::VmSpec = serde_json::from_str(&content).expect("malformed config");
    match vm_main(&args, &vm) {
        Err(error) => println!("vmrun exited with error: {}", error),
        Ok(_) => ()
    }
}
