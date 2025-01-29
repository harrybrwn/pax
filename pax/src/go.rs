use std::env::current_dir;
use std::path::PathBuf;
use std::process::Command;
use std::str;

use anyhow::{anyhow, Result};

use crate::git;

#[derive(Clone, pax_derive::FromLuaTable, pax_derive::IntoLua)]
pub(crate) struct Go<'lua> {
    pub(crate) root: String,
    pub(crate) cmd: Option<String>,
    pub(crate) out: Option<String>,
    mode: Option<String>,
    #[lua_default(true)]
    trimpath: bool,
    #[lua_default(Some(["-s", "-w"].map(String::from).to_vec()))]
    ldflags: Option<Vec<String>>,
    asmflags: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    compiler: Option<String>,
    /// Run 'go generate ./...' before running build commands
    generate: bool,
    // build_ldflags: Option<mlua::Value<'lua>>,
    build_ldflags: Option<mlua::Function<'lua>>,

    pub(crate) bin_access_mode: Option<u32>,
}

impl<'lua> Go<'lua> {
    pub(crate) fn list(&self) -> Result<String> {
        run_cmd(Command::new("go").args(["list", "-C", &self.dir()?]))
    }

    pub(crate) fn build(&self) -> Result<()> {
        if self.generate {
            self.generate(&Some("./...".to_string()))?;
        }
        let dir = self.dir()?;
        let mut args = vec!["-C", &dir, "build"];
        let out = self.out();
        if let Some(ref o) = out {
            args.push("-o");
            args.push(o);
        }
        if let Some(ref mode) = self.mode {
            args.push("-mod");
            args.push(mode);
        }
        let asmflags = self.asmflags.as_ref().map(|f| f.join(" "));
        if let Some(ref asmflags) = asmflags {
            args.push("-asmflags");
            args.push(asmflags);
        }
        let tags = self.tags.as_ref().map(|t| t.join(","));
        if let Some(ref tags) = tags {
            args.push("-tags");
            args.push(tags);
        }
        if let Some(c) = &self.compiler {
            args.push("-compiler");
            args.push(c);
        }
        if self.trimpath {
            args.push("-trimpath");
        }
        let mut ldflags = Vec::<String>::new();
        if let Some(explicit_ldflags) = self.ldflags.clone() {
            ldflags.extend(explicit_ldflags);
        }
        if let Some(ref build_ldflags) = self.build_ldflags {
            let extras: Vec<String> = build_ldflags.call(GoBuildData::new(&dir)?)?;
            ldflags.extend(extras);
        }
        let ldflags_flag = ldflags.join(" ");
        if !ldflags.is_empty() {
            args.push("-ldflags");
            args.push(&ldflags_flag);
        }
        // cmd MUST be added last
        if let Some(cmd) = &self.cmd {
            args.push(cmd);
        }
        println!("go {}", args.join(" "));
        let out = Command::new("go").args(args).output()?;
        if !out.status.success() {
            let s = str::from_utf8(&out.stderr).map(|s| s.strip_suffix('\n').unwrap_or(s))?;
            return Err(anyhow!("{}", s));
        }
        Ok(())
    }

    pub(crate) fn run(&self) -> Result<()> {
        let dir = self.dir()?;
        let mut args = vec!["-C", &dir, "run"];
        if let Some(ref mode) = self.mode {
            args.push("-mod");
            args.push(mode);
        }
        let ldflags = self.ldflags.as_ref().map(|f| f.join(" "));
        if let Some(ref ldflags) = ldflags {
            args.push("-ldflags");
            args.push(ldflags);
        }
        let asmflags = self.asmflags.as_ref().map(|f| f.join(" "));
        if let Some(ref asmflags) = asmflags {
            args.push("-asmflags");
            args.push(asmflags);
        }
        let tags = self.tags.as_ref().map(|t| t.join(","));
        if let Some(ref tags) = tags {
            args.push("-tags");
            args.push(tags);
        }
        if let Some(c) = &self.compiler {
            args.push("-compiler");
            args.push(c);
        }
        if self.trimpath {
            args.push("-trimpath");
        }
        if let Some(cmd) = &self.cmd {
            args.push(cmd);
        }
        println!("go {}", args.join(" "));
        let out = Command::new("go").args(args).output()?;
        if !out.status.success() {
            let s = str::from_utf8(&out.stderr).map(|s| s.strip_suffix('\n').unwrap_or(s))?;
            return Err(anyhow!("{}", s));
        }
        Ok(())
    }

    pub(crate) fn generate(&self, cmd: &Option<String>) -> Result<()> {
        let dir = self.dir()?;
        let mut args = vec!["-C", &dir, "generate"];
        let tags = self.tags.as_ref().map(|t| t.join(","));
        if let Some(ref tags) = tags {
            args.push("-tags");
            args.push(tags);
        }
        if let Some(cmd) = &cmd {
            args.push(cmd);
        }
        println!("go {}", args.join(" "));
        let out = Command::new("go")
            .args(args)
            .stdout(std::io::stdout())
            .output()?;
        if !out.status.success() {
            let s = str::from_utf8(&out.stderr).map(|s| s.strip_suffix('\n').unwrap_or(s))?;
            return Err(anyhow!("{}", s));
        }
        Ok(())
    }

    fn dir(&self) -> Result<String> {
        let root = PathBuf::from(&self.root);
        let dir = if root.is_relative() {
            std::env::current_dir()?.join(root)
        } else {
            root
        };
        Ok(String::from(dir.to_str().ok_or_else(|| {
            anyhow::anyhow!("failed to convert root to to string")
        })?))
    }

    fn out(&self) -> Option<String> {
        if let Some(ref out) = self.out {
            let cwd = current_dir().ok()?;
            let p = PathBuf::from(out);
            let dir = if p.is_relative() { cwd.join(p) } else { p };
            Some(String::from(dir.to_str()?))
        } else {
            None
        }
    }

    fn from_dir(d: &str) -> Self {
        Self {
            root: d.to_string(),
            cmd: None,
            out: None,
            mode: None,
            ldflags: Some(["-s", "-w"].map(String::from).to_vec()),
            asmflags: None,
            trimpath: true,
            tags: None,
            compiler: None,
            generate: false,
            build_ldflags: None,
            bin_access_mode: None,
        }
    }

    pub fn name(&self) -> Option<String> {
        if let Some(cmd) = &self.cmd {
            PathBuf::from(cmd)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
        } else {
            PathBuf::from(&self.root)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
        }
    }
}

impl<'lua> mlua::FromLua<'lua> for Go<'lua> {
    fn from_lua(
        value: mlua::prelude::LuaValue<'lua>,
        lua: &'lua mlua::prelude::Lua,
    ) -> mlua::prelude::LuaResult<Self> {
        use mlua::Value;
        match value {
            Value::Nil => Ok(Self::from_dir(".")),
            Value::String(s) => Ok(Self::from_dir(s.to_str()?)),
            Value::Table(t) => Self::from_lua_table(t, lua),
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: std::any::type_name::<Self>(),
                message: None,
            }),
        }
    }
}

fn run_cmd(cmd: &mut Command) -> Result<String> {
    let out = cmd.output()?;
    if !out.status.success() {
        let s = str::from_utf8(&out.stderr).map(|s| s.strip_suffix('\n').unwrap_or(s))?;
        return Err(anyhow!("{}", s));
    }
    match str::from_utf8(&out.stdout)?.strip_suffix('\n') {
        None => Err(anyhow!("no output from command")),
        Some(s) => Ok(String::from(s)),
    }
}

#[derive(pax_derive::FromLua, pax_derive::IntoLua)]
struct GoBuildData {
    git_sha: String,
    date: String,
}

impl GoBuildData {
    fn new(dir: &str) -> Result<Self> {
        Ok(Self {
            git_sha: git::head(dir)?,
            date: chrono::Local::now().to_rfc3339(),
        })
    }
}
