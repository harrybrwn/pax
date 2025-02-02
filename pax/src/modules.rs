use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use mlua::Lua;
use which::which;

use crate::dl;
use crate::git;
use crate::git::GitCloneOpts;
use crate::go::Go;
use crate::os::{exec, ExecOptions};
use crate::util::{gcc_features, get_user_email, get_user_name, git_version};

macro_rules! sub_module {
    ($name:ident; $($func:ident),*) => {
        sub_module!(@table $name; $($func),*);
    };
    (@table $name:ident; $($func:ident),*) => {
        #[derive(Clone, Debug, Default)]
        pub(crate) struct $name;
        impl ::mlua::IntoLua<'_> for $name {
            fn into_lua(self, lua: &'_ ::mlua::Lua) -> mlua::prelude::LuaResult<mlua::prelude::LuaValue<'_>> {
                let t = lua.create_table()?;
                $(
                    t.set(stringify!($func), lua.create_function(Self::$func)?)?;
                )*
                Ok(::mlua::Value::Table(t))
            }
        }
    };
    (@userdata $name:ident $(;)? $($func:ident),*) => {
        #[derive(Clone, Debug, Default)]
        pub(crate) struct $name;
        impl ::mlua::UserData for $name {
            fn add_methods<'lua, M: ::mlua::prelude::LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
                $(
                    methods.add_function(stringify!($func), Self::$func)
                );*
            }
        }
    }
}

sub_module!(@userdata GitSubModule; email, username, clone, version);

impl GitSubModule {
    fn email(_lua: &mlua::Lua, _: ()) -> mlua::Result<String> {
        Ok(get_user_email()?)
    }
    fn username(_lua: &mlua::Lua, _: ()) -> mlua::Result<String> {
        Ok(get_user_name()?)
    }
    fn version(_lua: &mlua::Lua, _: ()) -> mlua::Result<String> {
        Ok(git_version()?)
    }
    fn clone(_lua: &mlua::Lua, (repo, opts): (String, Option<GitCloneOpts>)) -> mlua::Result<()> {
        git::git_clone(opts.unwrap_or_else(|| GitCloneOpts::new(repo)))
            .map_err(mlua::Error::runtime)?;
        Ok(())
    }
}

sub_module!(@userdata CargoModule; build);

impl CargoModule {
    fn build(
        lua: &mlua::Lua,
        (args, _opts): (mlua::Value, Option<mlua::Table<'_>>),
    ) -> mlua::Result<()> {
        use super::crates;
        use mlua::FromLua;
        let cargo = match &args {
            mlua::Value::Table(tbl) => match tbl.get::<_, String>(1) {
                Ok(s) => crates::Cargo::from_path_and_table(&s, tbl)?,
                Err(_) => crates::Cargo::from_lua(args, lua)?,
            },
            mlua::Value::String(s) => crates::Cargo::from_path(s.to_str()?),
            mlua::Value::Nil => crates::Cargo::from_path("."),
            _ => crates::Cargo::from_lua(args, lua)?,
        };
        println!("building {}", cargo.root);
        let res = cargo.build();
        match res {
            Err(e) => {
                println!("{:?}", e);
                Err(mlua::Error::runtime(e))
            }
            Ok(_) => Ok(()),
        }
    }
}

sub_module!(@userdata GoModule; list, build, run, generate);

impl GoModule {
    fn list(_lua: &mlua::Lua, go: Go) -> mlua::Result<String> {
        go.list().map_err(mlua::Error::runtime)
    }

    fn build(_lua: &mlua::Lua, go: Go) -> mlua::Result<()> {
        go.build().map_err(mlua::Error::runtime)
    }

    fn run(_lua: &mlua::Lua, go: Go) -> mlua::Result<()> {
        go.run().map_err(mlua::Error::runtime)
    }

    fn generate(_lua: &mlua::Lua, go: Go) -> mlua::Result<()> {
        go.generate(&go.cmd).map_err(mlua::Error::runtime)
    }
}

sub_module!(@userdata DlModule; fetch, kubectl, jq, youtube_dl, yt_dlp, mc, tetris, balena_etcher);

impl DlModule {
    fn fetch(_lua: &mlua::Lua, (url, opts): (String, dl::DownloadOpts)) -> mlua::Result<()> {
        dl::fetch(url, opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
    fn kubectl(_lua: &mlua::Lua, opts: dl::DownloadOpts) -> mlua::Result<()> {
        dl::kubectl(opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
    fn jq(_: &mlua::Lua, opts: dl::DownloadOpts) -> mlua::Result<()> {
        dl::jq(opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
    fn youtube_dl(_: &Lua, opts: dl::DownloadOpts) -> mlua::Result<()> {
        dl::youtube_dl(opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
    fn yt_dlp(_: &Lua, opts: dl::DownloadOpts) -> mlua::Result<()> {
        dl::yt_dlp(opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
    fn mc(_: &Lua, opts: dl::DownloadOpts) -> mlua::Result<()> {
        dl::mc(opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
    fn tetris(_: &Lua, opts: dl::DownloadOpts) -> mlua::Result<()> {
        dl::tetris(opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
    fn balena_etcher(_: &Lua, opts: dl::DownloadOpts) -> mlua::Result<()> {
        dl::balena_etcher(opts).map_err(mlua::Error::runtime)?;
        Ok(())
    }
}

sub_module!(@userdata PathMod; join, is_absolute, is_relative, parent, basename);

impl PathMod {
    fn join(_: &Lua, args: mlua::Variadic<mlua::Value>) -> mlua::Result<String> {
        Ok(path_from_lua(args)?.to_string_lossy().to_string())
    }

    fn is_absolute(_: &Lua, args: mlua::Variadic<mlua::Value>) -> mlua::Result<bool> {
        Ok(path_from_lua(args)?.is_absolute())
    }

    fn is_relative(_: &Lua, args: mlua::Variadic<mlua::Value>) -> mlua::Result<bool> {
        Ok(path_from_lua(args)?.is_relative())
    }

    fn parent(_: &Lua, args: mlua::Variadic<mlua::Value>) -> mlua::Result<String> {
        let buf = path_from_lua(args)?;
        Ok(buf
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .to_string())
    }

    fn basename(_: &Lua, s: String) -> mlua::Result<Option<String>> {
        let name = Path::new(&s)
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| Some(String::from(n)));
        Ok(name)
    }
}

sub_module!(@userdata FSMod; exists, rm, rmdir, rmdir_all, mkdir, mkdir_all, mkdir_force, stat);

impl FSMod {
    fn exists(_: &Lua, args: mlua::Variadic<mlua::Value>) -> mlua::Result<bool> {
        Ok(path_from_lua(args)?.exists())
    }

    fn rm(_: &Lua, args: mlua::Variadic<String>) -> mlua::Result<()> {
        for path in args {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn rmdir(_: &Lua, args: mlua::Variadic<String>) -> mlua::Result<()> {
        for a in args {
            fs::remove_dir(a)?;
        }
        Ok(())
    }

    fn rmdir_all(_: &Lua, args: mlua::Variadic<String>) -> mlua::Result<()> {
        for a in args {
            fs::remove_dir_all(a)?;
        }
        Ok(())
    }

    fn mkdir(_: &Lua, args: mlua::Variadic<String>) -> mlua::Result<()> {
        for a in args {
            fs::create_dir(a)?;
        }
        Ok(())
    }

    fn mkdir_all(_: &Lua, args: mlua::Variadic<String>) -> mlua::Result<()> {
        for a in args {
            fs::create_dir_all(a)?;
        }
        Ok(())
    }

    fn mkdir_force(_: &Lua, args: mlua::Variadic<String>) -> mlua::Result<()> {
        for path in args {
            match fs::create_dir(path) {
                Ok(_) => (),
                Err(e) => return Err(e.into()),
            };
        }
        Ok(())
    }

    fn stat(lua: &Lua, dir: String) -> mlua::Result<mlua::Table> {
        let stat = fs::metadata(dir)?;
        let t = lua.create_table()?;
        t.set("size", stat.size())?;
        t.set("mode", stat.mode())?;
        t.set("mtime", stat.mtime())?;
        t.set("atime", stat.atime())?;
        t.set("ctime", stat.ctime())?;
        t.set("uid", stat.uid())?;
        t.set("gid", stat.gid())?;
        t.set("dev", stat.dev())?;
        t.set("ino", stat.ino())?;
        t.set("blocks", stat.blocks())?;
        t.set("blksize", stat.blksize())?;
        t.set("nlink", stat.nlink())?;
        Ok(t)
    }
}

fn path_from_lua(args: mlua::Variadic<mlua::Value>) -> mlua::Result<PathBuf> {
    use mlua::{Error, Value};
    let mut buf = PathBuf::new();
    for arg in args.iter() {
        buf.push(match arg {
            Value::String(s) => {
                let st =
                    s.to_str()
                        .ok()
                        .map(|s| s.to_string())
                        .ok_or(Error::FromLuaConversionError {
                            from: "string",
                            to: "Path",
                            message: Some("failed to get string".to_string()),
                        });
                st
            }
            Value::Integer(n) => Ok(n.to_string()),
            Value::Number(n) => Ok(n.to_string()),
            Value::Nil => Err(Error::FromLuaConversionError {
                from: "nil",
                to: "PathBuf",
                message: Some("cannot join nil in a file path".to_string()),
            }),
            v => Err(Error::FromLuaConversionError {
                from: v.type_name(),
                to: "PathBuf",
                message: Some(format!("cannot join {} in a file path", v.type_name())),
            }),
        }?);
    }
    Ok(buf
        .components()
        .enumerate()
        .filter_map(|(i, comp)| match comp {
            std::path::Component::CurDir if i > 0 => None,
            c => Some(c),
        })
        .collect())
}

sub_module!(@userdata OsMod; exec, which, libc_version);

#[derive(Debug, Default, pax_derive::FromLuaTable, pax_derive::IntoLua)]
struct LibcVersion {
    major: u32,
    minor: u32,
}

impl OsMod {
    fn exec(
        _: &mlua::Lua,
        (bin, args, opts): (String, Option<Vec<String>>, Option<ExecOptions>),
    ) -> mlua::Result<i32> {
        Ok(exec(bin, args.unwrap_or(Vec::new()), opts)?)
    }

    fn which(_: &mlua::Lua, name: String) -> mlua::Result<String> {
        Ok(which(name)
            .map_err(|e| mlua::Error::runtime(e))?
            .to_str()
            .unwrap_or("")
            .into())
    }

    fn libc_version(_: &mlua::Lua, _: ()) -> mlua::Result<LibcVersion> {
        let f = gcc_features().map_err(|e| mlua::Error::runtime(e))?;
        Ok(LibcVersion {
            major: f.glibc_version_major,
            minor: f.glibc_version_minor,
        })
    }
}
