
use command_macros::cmd;

pub fn get_tap_ifaces() -> Result<Vec<String>, &'static str> {
    let output = cmd!(ifconfig ("-g") tap).output()
        .map_err(|_| "cannot spawn ifconfig")?;
    let stdout = std::str::from_utf8(&output.stdout)
        .map_err(|_| "cannot decode `ifconfig -g tap` output")?;
    Ok(stdout.lines().map(|s| s.to_string()).collect())
}

// naive way to determine if the tap interface is opened, this has few critical
// flews, for example if the interface is not a tap interface at all this will
// also report true
pub fn is_tap_opened(name: &str) -> Result<bool, &'static str> {
    let output = cmd!(ifconfig (name)).output()
        .map_err(|_| "cannot spawn ifconfig $name")?;
    let stdout = std::str::from_utf8(&output.stdout)
        .map_err(|_| "cannot decode `ifconfig $name` output")?;

    Ok(stdout.contains("Opened by PID"))
}
