mod build;
mod crates;
mod deb;
mod dl;
mod error;
mod git;
mod go;
mod modules;
mod os;
mod project;
mod util;

use std::{cell::RefCell, fs, io::Read, rc::Rc};

use clap::{Parser, Subcommand};
use mlua::Lua;
use util::{scdoc, SCDocOpts};

use crate::build::{BuildSpec, RefCellBuildSpec, DEFAULT_DIST};
use crate::modules::GitSubModule;
use crate::util::{lua_octal, print_function};

#[derive(Default, Debug, Parser)]
struct Cli {
    #[arg(long, short, default_value = "pax.lua")]
    config: String,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Default, Subcommand)]
enum Command {
    /// Manage configuration
    Config,
    /// Tests
    #[clap(hide = true)]
    Test,
    /// Run the cli
    #[default]
    Run,
}

impl Cli {
    fn run(&self, lua: &Lua) -> mlua::Result<()> {
        let mut file = std::fs::File::options()
            .write(false)
            .read(true)
            .create(false)
            .open(&self.config)
            .map_err(|e| {
                mlua::Error::runtime(format!(
                    "could not open config file {:?}: {}",
                    self.config, e
                ))
            })?;
        self.process(lua, &mut file)?;
        Ok(())
    }

    fn process<R>(&self, lua: &Lua, configbody: &mut R) -> mlua::Result<PaxConfig>
    where
        R: Read,
    {
        let mut body = String::new();
        configbody.read_to_string(&mut body)?;
        let rt_conf = Rc::new(RefCell::new(PaxConfig::default()));
        lua.globals()
            .set("octal", lua.create_function(lua_octal)?)?;
        let package: mlua::Table = lua.globals().get("package")?;
        let loaded: mlua::Table = package.get("loaded")?;
        loaded.set("pax", rt_conf.clone())?; // use require('pax') to access
        lua.load(body)
            .set_name(&self.config)
            .set_mode(mlua::ChunkMode::Text)
            .exec()?;
        Ok(rt_conf.take())
    }
}

#[derive(Debug, Default)]
struct PaxOptions {
    files_base: Option<String>,
    dist: Option<String>,
}

#[derive(Debug, Default)]
struct PaxConfig {
    opts: PaxOptions,
    specs: Vec<BuildSpec>,
    spec: BuildSpec,
}

impl mlua::UserData for PaxConfig {
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        macro_rules! gen_userdata_getset {
            ($name:ident, $($more:ident),+) => {
                gen_userdata_getset!($name);
                gen_userdata_getset!($($more),+);
            };
            ($(@ $field:ident)?, $name:ident, $($more:ident),+) => {
                gen_userdata_getset!($(@$field,)? $name);
                gen_userdata_getset!($(@$field,)? $($more),+);
            };
            // Use '@<field>' to turn the name into a nested field.
            // This allows you to set a nested field in rust land.
            ($(@ $field:ident,)? $name:ident) => {
               fields.add_field_method_get(stringify!($name), |_lua, this| {
                   Ok(this.$($field.)?$name.clone())
               });
               fields.add_field_method_set(stringify!($name), |_lua, this, val| {
                   this.$($field.)?$name = val;
                   Ok(())
               });
            }
        }
        gen_userdata_getset!(@opts, files_base, dist);
        fields.add_field_method_get("specs", PaxConfig::get_specs);
        fields.add_field("git", GitSubModule);
        fields.add_field("cargo", modules::CargoModule);
        fields.add_field("go", modules::GoModule);
        fields.add_field("dl", modules::DlModule);
        fields.add_field("path", modules::PathMod);
        fields.add_field("fs", modules::FSMod);
        fields.add_field("os", modules::OsMod);
        fields.add_field("Urgency", deb::Urgency::Low); // adds all variants
        fields.add_field("Priority", deb::Priority::default());
    }

    fn add_methods<'lua, M: mlua::prelude::LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_function("print", print_function);
        methods.add_function("log", Self::func_log);
        methods.add_method_mut("add", Self::method_add_spec);
        methods.add_method_mut("package", Self::method_package);
        methods.add_method_mut("package_crate", Self::method_build_crate);
        methods.add_method("packages", |_lua, this, ()| Ok(this.specs.clone()));
        methods.add_function("octal", lua_octal);
        methods.add_function("new_spec", Self::func_new_spec);
        methods.add_function("table_extend", Self::func_table_extend);
        methods.add_function("exec", Self::func_exec);
        methods.add_function("sh", Self::func_sh);
        methods.add_function("in_dir", Self::func_in_dir);
        methods.add_function("cwd", |_, ()| {
            Ok(std::env::current_dir()?.to_string_lossy().to_string())
        });
        methods.add_function("project", |_, spec: BuildSpec| {
            let p = project::Project::from_spec(spec);
            p.validate_version()?;
            Ok(p)
        });
        methods.add_function("scdoc", |_, opts: SCDocOpts| Ok(scdoc(opts)?));
    }
}

impl<'lua> PaxConfig {
    #[inline]
    fn get_specs(lua: &'lua Lua, this: &Self) -> mlua::Result<mlua::Table<'lua>> {
        lua.create_sequence_from(this.specs.clone())
    }

    fn func_new_spec(_lua: &mlua::Lua, s: RefCellBuildSpec) -> mlua::Result<BuildSpec> {
        s.borrow_mut().pre_process(Some("dist".to_string()))?;
        Ok(s.take())
    }

    fn func_exec(
        _lua: &mlua::Lua,
        (cmd, vargs): (String, Option<Vec<String>>),
    ) -> mlua::Result<()> {
        std::process::Command::new(cmd)
            .args(vargs.unwrap_or(vec![]))
            .stderr(std::io::stderr())
            .stdout(std::io::stdout())
            .output()?;
        Ok(())
    }

    fn func_sh(_: &mlua::Lua, script: String) -> mlua::Result<()> {
        std::process::Command::new("sh")
            .args(["-c", script.as_str()])
            .stderr(std::io::stderr())
            .stdout(std::io::stdout())
            .output()?;
        Ok(())
    }

    fn func_table_extend(
        _lua: &mlua::Lua,
        (list, a): (mlua::Table, mlua::Table),
    ) -> mlua::Result<()> {
        let mut ix = list.len()? + 1;
        let pairs = a.pairs::<i64, mlua::Value>();
        for pair in pairs {
            let (_, v) = pair?;
            list.set(ix, v)?;
            ix += 1;
        }
        Ok(())
    }

    fn func_in_dir(_lua: &mlua::Lua, (dir, func): (String, mlua::Function)) -> mlua::Result<()> {
        let cwd = std::env::current_dir()?;
        std::env::set_current_dir(dir)?;
        let res = func.call::<_, ()>(());
        std::env::set_current_dir(cwd)?;
        res
    }

    fn func_log(_: &mlua::Lua, msg: String) -> mlua::Result<()> {
        println!("[\x1b[01;32mpax\x1b[0m] {}", msg);
        Ok(())
    }

    fn method_package(_lua: &mlua::Lua, this: &mut Self, s: RefCellBuildSpec) -> mlua::Result<()> {
        let dist = this
            .opts
            .dist
            .to_owned()
            .unwrap_or(DEFAULT_DIST.to_string());
        _ = std::fs::create_dir_all(&dist); // ignore error
        s.borrow_mut().pre_process(this.opts.files_base.clone())?;
        s.borrow_mut().build(dist)?;
        this.specs.push(s.take());
        Ok(())
    }

    fn method_build_crate(
        lua: &mlua::Lua,
        this: &mut Self,
        (p, overrides): (String, Option<mlua::Table>),
    ) -> mlua::Result<()> {
        let path = std::path::Path::new(&p);
        let mut s = String::new();
        fs::File::open(path.join("Cargo.toml"))?.read_to_string(&mut s)?;
        let conf = s.parse::<toml::Table>().map_err(mlua::Error::runtime)?;
        let ovrd = match overrides {
            None => lua.create_table()?,
            Some(o) => o,
        };
        let mut spec = BuildSpec::from_toml_with_overrides(conf, ovrd)?;
        let dist = this
            .opts
            .dist
            .to_owned()
            .unwrap_or(DEFAULT_DIST.to_string());
        _ = std::fs::create_dir_all(&dist); // ignore error
        spec.pre_process(this.opts.files_base.clone())?;
        spec.build(dist)?;
        this.specs.push(spec);
        Ok(())
    }

    fn method_add_spec(_lua: &mlua::Lua, this: &mut Self, s: RefCellBuildSpec) -> mlua::Result<()> {
        s.borrow_mut().merge_in(&this.spec);
        s.borrow_mut().pre_process(this.opts.files_base.clone())?;
        this.specs.push(s.take());
        Ok(())
    }
}

fn main() {
    let cli = Cli::parse();
    let lua = Lua::new();
    match &cli.command {
        Some(Command::Config) => {
            let res = std::fs::read(&cli.config).unwrap();
            let s = String::from_utf8(res).unwrap();
            println!("{s}");
        }
        Some(Command::Test) => {}
        _ => {
            match cli.run(&lua) {
                Err(e) => println!("Error: {}", e),
                Ok(_) => (),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::build::File;
    use crate::deb::{Priority, Urgency};
    use crate::Cli;
    use core::panic;
    use mlua::Lua;

    #[test]
    fn config() {
        // let p = std::path::Path::new("./repos/exa/target/release/exa");
        // println!("{:?}", p);
        let lua = Lua::new();
        let cli = Cli::default();
        let mut body = r#"
            local pax = require("pax")
            local pkg = {
                name = 'test',
                package = 'x',
                version = "v0.1",
                arch = 'amd64',
                author = 'jerry',
                email = 'jerry@jerry.se',
                priority = "required",
                files={
                  'one','two','three',
                  {src='a',dst='/a'},
                  { src = "path/to/file", dst = "/usr/share/file" },
                  'a:b'
                },
                dependencies = {'a', 'b'},
                recommends = {'c', 'd'},
            }
            pax.files_base = "/usr/share"
            pax:add(pkg)
        "#
        .as_bytes();
        let res = cli.process(&lua, &mut body).unwrap();
        let spec = &res.specs[0];
        assert_eq!(
            spec.files,
            vec![
                File::new("one", "/usr/share/one"),
                File::new("two", "/usr/share/two"),
                File::new("three", "/usr/share/three"),
                File::new("a", "/a"),
                File::new("path/to/file", "/usr/share/file"),
                File::new("a", "b"),
            ]
        );
        assert_eq!(spec.name, Some("test".to_string()));
        assert_eq!(spec.version, "v0.1".to_string());
        assert_eq!(spec.author, Some("jerry".to_string()));
        assert_eq!(spec.email, Some("jerry@jerry.se".to_string()));
        assert_eq!(spec.dependencies, &["a", "b"]);
        assert_eq!(
            spec.recommends,
            Some(Vec::from(&["c", "d"].map(|s| s.to_string())))
        );
        assert_eq!(spec.priority, Priority::Required);
        assert_eq!(spec.arch, "amd64");
        assert_eq!(res.opts.files_base, Some("/usr/share".to_string()));
    }

    #[test]
    fn add() {
        let lua = Lua::new();
        let cli = Cli::default();
        let mut body = r#"
        local pax = require("pax")
        pax:add({
          name = "sub-package",
          package = 'sub-package',
          version = "1.0.0",
          arch = "x86_64",
          email = nil,
          author = 'jim',
          description = "test build object",
          urgency = pax.Urgency.Critical,
          files = {
            "one.txt",
            "two.txt",
            "three.txt",
            {
              src = "Cargo.toml",
              dst = "/usr/share/Cargo.toml",
            },
          },
        })"#
        .as_bytes();
        let res = match cli.process(&lua, &mut body) {
            Err(e) => {
                println!("{:?}", e);
                panic!("{}", e);
            }
            Ok(cfg) => cfg,
        };
        let s = &res.specs[0];
        assert_eq!(res.specs.len(), 1);
        assert_eq!(s.author, Some("jim".to_string()));
        assert_eq!(s.email, None);
        assert_eq!(s.priority, Priority::Optional);
        assert_eq!(s.name, Some("sub-package".to_string()));
        assert_eq!(s.version, "1.0.0".to_string());
        assert_eq!(s.urgency, Some(Urgency::Critical));
        assert_eq!(s.arch, "x86_64");
    }

    #[test]
    fn derives() {
        use pax_derive::{FromLua, IntoLua, LuaGettersSetters, UserData};
        #[derive(LuaGettersSetters, IntoLua, FromLua, Clone)]
        struct A {
            a: String,
            b: usize,
        }
        let mut t = A {
            a: String::from("this is 'a'"),
            b: 69,
        };
        let lua = Lua::new();
        let a = A::get_a(&lua, &t).unwrap();
        let b = A::get_b(&lua, &t).unwrap();
        assert_eq!(a, "this is 'a'");
        assert_eq!(b, 69);
        A::set_a(&lua, &mut t, "x".to_string()).unwrap();
        A::set_b(&lua, &mut t, 10).unwrap();
        assert_eq!(t.a, "x");
        assert_eq!(t.b, 10);

        #[derive(LuaGettersSetters, IntoLua, FromLua, Clone)]
        struct B(String, usize);
        let mut t = B("this is 'a'".to_string(), 69);
        let lua = Lua::new();
        let a = B::get_0(&lua, &t).unwrap();
        let b = B::get_1(&lua, &t).unwrap();
        assert_eq!(a, "this is 'a'");
        assert_eq!(b, 69);
        B::set_0(&lua, &mut t, "x".to_string()).unwrap();
        B::set_1(&lua, &mut t, 10).unwrap();
        assert_eq!(t.0, "x");
        assert_eq!(t.1, 10);

        #[derive(Debug, UserData)]
        enum Data {
            A,
            B,
        }
    }
}
