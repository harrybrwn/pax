#[derive(Clone, Debug, Default, pax_derive::FromLuaTable, pax_derive::IntoLua)]
pub(crate) struct ExecOptions {
    dir: Option<String>,
    stdin_file: Option<String>,
    stdout_file: Option<String>,
}

impl mlua::FromLua<'_> for ExecOptions {
    fn from_lua(
        value: mlua::prelude::LuaValue<'_>,
        lua: &'_ mlua::prelude::Lua,
    ) -> mlua::prelude::LuaResult<Self> {
        use mlua::Value;
        match value {
            Value::Nil => Ok(Self::default()),
            Value::Table(t) => Self::from_lua_table(t, lua),
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: std::any::type_name::<Self>(),
                message: None,
            }),
        }
    }
}

pub(crate) fn exec(bin: String, args: Vec<String>, opts: Option<ExecOptions>) -> mlua::Result<i32> {
    let mut cmd = &mut std::process::Command::new(bin);
    if args.len() > 0 {
        cmd = cmd.args(args);
    }
    if let Some(opts) = opts {
        if let Some(dir) = opts.dir {
            cmd = cmd.current_dir(dir);
        }
        if let Some(fname) = opts.stdout_file {
            let file = std::fs::File::options()
                .write(true)
                .create(true)
                .open(fname)?;
            cmd = cmd.stdout(file);
        } else {
            cmd = cmd.stdout(std::io::stdout());
        }
        if let Some(fname) = opts.stdin_file {
            let file = std::fs::File::options()
                .read(true)
                .write(false)
                .create_new(false)
                .open(fname)?;
            cmd = cmd.stdin(file);
        }
    } else {
        cmd = cmd.stdout(std::io::stdout());
    }
    cmd = cmd.stderr(std::io::stderr());
    let out = cmd.output()?;
    Ok(out.status.code().unwrap_or(0))
}
