pub type Recovery = dyn Fn();

pub enum Assertion {
    /// An error that fixes are possible and can be performed by the supervisor
    Recoverable(String, String, Box<Recovery>),
    /// An error that is fatal and require explicit operator action to clear
    Fatal(String, String),
    /// This is a combination of list of other assertions
    Container(Vec<(String, Assertion)>),
}

impl std::fmt::Display for Assertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Assertion::Recoverable(scope, description, ..) => f
                .debug_tuple("Assertion::Recoverable")
                .field(&scope)
                .field(&description)
                .finish(),
            Assertion::Fatal(scope, description) => f
                .debug_tuple("Assertion::Fatal")
                .field(&scope)
                .field(&description)
                .finish(),
            Assertion::Container(items) => {
                let mut debug = f.debug_struct("Assertion::Container");
                for (key, assertion) in items.iter() {
                    // Although the inner type is Vec<(String, _)>, in practice the
                    // key value pair should not have duplicated key
                    debug.field(key.as_str(), assertion);
                }
                debug.finish()
            }
        }
    }
}

impl std::fmt::Debug for Assertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Assertion::Recoverable(scope, description, ..) => f
                .debug_tuple("Assertion::Recoverable")
                .field(&scope)
                .field(&description)
                .finish(),
            Assertion::Fatal(scope, description) => f
                .debug_tuple("Assertion::Fatal")
                .field(&scope)
                .field(&description)
                .finish(),
            Assertion::Container(items) => {
                let mut debug = f.debug_struct("Assertion::Container");
                for (key, assertion) in items.iter() {
                    // Although the inner type is Vec<(String, _)>, in practice the
                    // key value pair should not have duplicated key
                    debug.field(key.as_str(), assertion);
                }
                debug.finish()
            }
        }
    }
}

impl Assertion {
    pub fn is_recoverable(&self) -> bool {
        match self {
            Assertion::Fatal(_, _) => false,
            Assertion::Recoverable(_, _, _) => true,
            Assertion::Container(list) => {
                for l in list.iter() {
                    if !l.1.is_recoverable() {
                        return false;
                    }
                }
                true
            }
        }
    }

    pub fn recovery_prompt(&self) -> String {
        let mut base = String::new();

        match self {
            Assertion::Fatal(_, _) => (),
            Assertion::Recoverable(obj, why, _) => base.push_str(format!("{obj}: {why}").as_str()),
            Assertion::Container(list) => {
                for l in list.iter() {
                    if !l.1.is_recoverable() {
                        return base;
                    } else {
                        base.push_str(format!("{}:", l.0).as_str());
                        for line in l.1.recovery_prompt().lines() {
                            base.push_str(format!("\n  {}", line).as_str());
                        }
                    }
                }
            }
        }

        base
    }

    pub fn recover(&self) {
        match self {
            Assertion::Fatal(_, _) => (),
            Assertion::Recoverable(_, _, f) => f(),
            Assertion::Container(list) => {
                for l in list.iter() {
                    l.1.recover();
                }
            }
        }
    }

    pub fn print(&self, scope: String) -> String {
        match self {
            Assertion::Fatal(loc, why) => format!("[fatal] {loc}: {why}"),
            Assertion::Recoverable(loc, why, _) => format!("[recoverable] {loc}: {why}"),
            Assertion::Container(list) => {
                let mut value = format!("{scope}\n");
                for i in 0..list.len() {
                    let (scope, assertion) = &list[i];

                    let prefix = if i == list.len() - 1 {
                        "└─".to_string()
                    } else {
                        "├─".to_string()
                    };

                    let p = assertion.print(scope.to_string());
                    let mut is_fst = false;

                    let pl: Vec<_> = p.lines().collect::<Vec<_>>();
                    for line in pl.iter() {
                        if !is_fst {
                            value.push_str(format!("{prefix}{line}\n").as_str());
                            is_fst = true;
                        } else {
                            let ins = if i == list.len() - 1 { " " } else { "|" };
                            value.push_str(format!("{ins}  {line}\n").as_str());
                        }
                    }
                }
                value
            }
        }
    }

    pub fn from_io_error(err: std::io::Error) -> Assertion {
        Assertion::Fatal("std::io::error".to_string(), format!("{:#?}", err))
    }
}
