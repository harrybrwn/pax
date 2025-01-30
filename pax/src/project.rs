use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use flate2::read::GzDecoder;
use md5::{Digest, Md5};
use mlua::FromLua;
use xz2::read::XzDecoder;

use crate::{
    build::{BuildSpec, File, DEFAULT_DIST},
    deb::Version,
    dl::{self, DownloadOpts},
    go::Go,
    util::{self, scdoc, SCDocOpts},
};

#[derive(Debug)]
pub(crate) struct Project {
    spec: BuildSpec,
    id: [u8; 16],
    base_dir: String,
    man_dir: String,
    build: Option<u32>,
}

impl Project {
    pub fn from_spec(spec: BuildSpec) -> Self {
        let mut md5 = Md5::new();
        _ = md5.write(spec.package.as_bytes());
        _ = md5.write(spec.version.as_bytes());
        let p = Self {
            spec,
            id: md5.finalize().into(),
            base_dir: "/usr".to_string(),
            man_dir: "/usr/share/man".to_string(),
            build: None,
        };
        _ = std::fs::create_dir_all(p.cache_dir());
        p
    }

    pub fn validate_version(&self) -> Result<(), io::Error> {
        let _ = Version::try_from(self.spec.version.as_ref())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("invalid version: {}", e)))?;
        Ok(())
    }
}

impl mlua::UserData for Project {
    fn add_fields<'lua, F: mlua::prelude::LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("base_dir", |_lua, this| Ok(this.base_dir.clone()));
        fields.add_field_method_set("base_dir", |_lua, this, val| {
            this.base_dir = val;
            Ok(())
        });
        fields.add_field_method_get("man_dir", |_lua, this| Ok(this.man_dir.clone()));
        fields.add_field_method_set("man_dir", |_, this, val| {
            this.man_dir = val;
            Ok(())
        });
        use mlua::Value;
        fields.add_field_method_get("version", |lua, this| lua.create_string(&this.spec.version));
        fields.add_field_method_get("package", |lua, this| lua.create_string(&this.spec.package));
        fields.add_field_method_get("arch", |lua, this| lua.create_string(&this.spec.arch));
        fields.add_field_method_get("essential", |_, this| Ok(this.spec.essential));
        fields.add_field_method_get("description", |lua, this| {
            Ok(match &this.spec.description {
                None => mlua::Value::Nil,
                Some(s) => mlua::Value::String(lua.create_string(s)?),
            })
        });
        fields.add_field_method_get("author", |lua, this| {
            Ok(match &this.spec.author {
                None => mlua::Value::Nil,
                Some(s) => mlua::Value::String(lua.create_string(s)?),
            })
        });
        fields.add_field_method_get("email", |lua, this| {
            Ok(match &this.spec.email {
                None => Value::Nil,
                Some(s) => Value::String(lua.create_string(s)?),
            })
        });
        fields.add_field_method_get("maintainer", |lua, this| {
            Ok(match &this.spec.maintainer {
                None => Value::Nil,
                Some(s) => Value::String(lua.create_string(s)?),
            })
        });
    }

    fn add_methods<'lua, M: mlua::prelude::LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("dir", |_, this, ()| {
            Ok(this.cache_dir().to_string_lossy().to_string())
        });
        methods.add_method("files", |_, this, ()| Ok(this.spec.files.clone()));
        methods.add_method_mut("add_binary", |_, this, val: String| this.add_bin(val));
        methods.add_method_mut("apt_source", |_, this, val| {
            if this.spec.apt_sources.is_none() {
                this.spec.apt_sources = Some(Vec::new());
            }
            this.spec.apt_sources.as_mut().unwrap().push(val);
            Ok(())
        });
        methods.add_method_mut("go_build", |_, this, opts: Go| {
            let mut opts = opts.clone();
            let name = opts
                .name()
                .ok_or(mlua::Error::runtime("could not find go build binary name"))?;
            if opts.out.is_none() {
                opts.out = Some(
                    this.cache_dir()
                        .join("bin")
                        .join(&name)
                        .to_string_lossy()
                        .to_string(),
                );
            }
            opts.build().map_err(mlua::Error::runtime)?;
            if let Some(mode) = opts.bin_access_mode {
                this.add_bin_mode(opts.out.unwrap(), mode)?;
            } else {
                this.add_bin(opts.out.unwrap())?;
            }
            Ok(())
        });
        methods.add_method_mut("cargo_build", |lua, this, args: mlua::Value| {
            use super::crates;
            let cargo = match &args {
                mlua::Value::Table(tbl) => match tbl.get::<_, String>(1) {
                    Ok(s) => crates::Cargo::from_path_and_table(&s, tbl)?,
                    Err(_) => crates::Cargo::from_lua(args, lua)?,
                },
                mlua::Value::String(s) => crates::Cargo::from_path(s.to_str()?),
                mlua::Value::Nil => crates::Cargo::from_path("."),
                _ => crates::Cargo::from_lua(args, lua)?,
            };
            cargo.build().map_err(mlua::Error::runtime)?;
            this.add_bin(cargo.bin())?;
            Ok(())
        });
        methods.add_method_mut("scdoc", |_, this, opts: SCDocOpts| this.scdoc(opts));
        methods.add_method_mut("build", |_, this, ()| this.build());
        methods.add_method_mut("finish", |_, this, ()| this.build());
        methods.add_method_mut("add_file", |_, this, args: mlua::Variadic<File>| {
            this.spec.files.extend(args);
            Ok(())
        });
        methods.add_method_mut("add_files", |_, this, args: Vec<File>| {
            this.spec.files.extend(args);
            Ok(())
        });
        methods.add_method_mut("merge_deb", |_, this, source: String| {
            this.merge_deb(&source)
        });
        methods.add_method_mut("reset_build_number", |_, this, ()| {
            this.set_build(0).map_err(|e| mlua::Error::runtime(e))
        });
        methods.add_method_mut("enable_auto_build_numbers", |_, this, ()| {
            this.init_build_no().map_err(|e| mlua::Error::runtime(e))
        });
        methods.add_method_mut("download_kubectl", |_, this, opts: DownloadOpts| {
            let mut opts = opts.clone();
            opts.out = Some(this.bin_path("kubectl"));
            let out = dl::kubectl(opts).map_err(mlua::Error::runtime)?;
            this.add_bin(out)?;
            Ok(())
        });
        methods.add_method_mut("download_jq", |_, this, opts: DownloadOpts| {
            let mut opts = opts.clone();
            opts.out = Some(this.bin_path("jq"));
            let out = dl::jq(opts).map_err(mlua::Error::runtime)?;
            this.add_bin(out)?;
            Ok(())
        });
        methods.add_method_mut("download_youtube_dl", |_, this, opts: DownloadOpts| {
            let mut opts = opts.clone();
            opts.out = Some(this.bin_path("youtube-dl"));
            let out = dl::youtube_dl(opts).map_err(mlua::Error::runtime)?;
            this.add_bin(out)?;
            Ok(())
        });
        methods.add_method_mut("download_yt_dlp", |_, this, opts: DownloadOpts| {
            let mut opts = opts.clone();
            opts.out = Some(this.bin_path("yt-dlp"));
            let out = dl::yt_dlp(opts).map_err(mlua::Error::runtime)?;
            this.add_bin(out)?;
            Ok(())
        });
        methods.add_method_mut("download_mc", |_, this, opts: DownloadOpts| {
            let mut opts = opts.clone();
            opts.out = Some(this.bin_path("mc"));
            let out = dl::mc(opts).map_err(mlua::Error::runtime)?;
            this.add_bin(out)?;
            Ok(())
        });
        methods.add_method_mut("download_tetris", |_, this, opts: DownloadOpts| {
            let mut opts = opts.clone();
            opts.out = Some(this.bin_path("tetris"));
            let out = dl::tetris(opts).map_err(mlua::Error::runtime)?;
            this.add_bin(out)?;
            Ok(())
        });
        methods.add_method_mut("download_balena_etcher", |_, this, opts: DownloadOpts| {
            let mut opts = opts.clone();
            opts.out = Some(this.bin_path("BalenaEtcher.AppImage"));
            let out = dl::balena_etcher(opts).map_err(mlua::Error::runtime)?;
            this.add_bin(out)?;
            Ok(())
        });
        methods.add_method_mut(
            "download_binary",
            |_, this, (url, name, opts): (String, Option<String>, Option<DownloadOpts>)| {
                this.download_binary(url, name, opts)
            },
        );
        methods.add_method("print", |_, this, ()| {
            println!("{:#?}", this);
            Ok(())
        });
    }
}

impl Project {
    fn cache_dir(&self) -> PathBuf {
        [
            ".pax/project".to_string(),
            self.spec.package.clone(),
            hex::encode(self.id),
        ]
        .iter()
        .collect()
    }

    fn add_bin<P: AsRef<Path>>(&mut self, val: P) -> mlua::Result<()> {
        self.add_bin_mode(val, 0o755)
    }

    fn add_bin_mode<P: AsRef<Path>>(&mut self, val: P, mode: u32) -> mlua::Result<()> {
        let p = val.as_ref();
        let name = p
            .file_name()
            .ok_or(mlua::Error::runtime("could not get file name"))?
            .to_string_lossy()
            .to_string();
        let dst: PathBuf = [&self.base_dir, "bin", &name].iter().collect();
        self.spec.files.push(File {
            src: val.as_ref().to_string_lossy().to_string(),
            dst: dst.to_string_lossy().to_string(),
            mode: Some(mode),
            dir: None,
        });
        Ok(())
    }

    fn build(&mut self) -> mlua::Result<()> {
        _ = std::fs::create_dir_all(DEFAULT_DIST);
        if let Some(n) = self.build {
            self.spec.buildno = Some(n);
            self.increment_build_no()?;
        }
        self.spec.pre_process(Some(self.base_dir.clone()))?;
        self.spec.build(DEFAULT_DIST.to_string())?;
        Ok(())
    }

    fn bin_path(&self, name: &str) -> String {
        let path = self.cache_dir().join("bin");
        if !path.exists() {
            _ = fs::create_dir_all(&path);
        }
        path.join(name).to_string_lossy().to_string()
    }

    fn scdoc(&mut self, opts: SCDocOpts) -> mlua::Result<()> {
        let mut opts = opts.clone();
        let name = if opts.output.len() == 0 {
            opts.input.replace(".scd", "")
        } else {
            opts.output.clone()
        };
        let dst = Path::new(&self.man_dir)
            .join(&opts.output)
            .to_string_lossy()
            .to_string();
        let src = self.cache_dir().join(&name).to_string_lossy().to_string();
        if let Some(parent) = Path::new(&src).parent() {
            _ = fs::create_dir_all(&parent);
        }
        opts.output = src.clone();
        scdoc(opts)?;
        self.spec.files.push(File {
            src,
            dst,
            mode: Some(0o644),
            dir: None,
        });
        Ok(())
    }

    fn merge_deb(&mut self, source: &str) -> mlua::Result<()> {
        let source_path = Path::new(source);
        let source_name = source_path
            .file_name()
            .ok_or(mlua::Error::runtime(
                "failed to get debian package filename",
            ))?
            .to_string_lossy()
            .to_string()
            .replace(".deb", "");
        let base = &self.cache_dir().join("debs").join(source_name);
        _ = fs::remove_dir_all(base);
        fs::create_dir_all(base).map_err(|e| {
            io::Error::new(e.kind(), format!("{}: failed to create dir {:?}", e, &base))
        })?;

        let mut pkg = ar::Archive::new(fs::File::open(source)?);
        while let Some(ar_entry) = pkg.next_entry() {
            let entry = ar_entry?;
            let name = String::from_utf8(entry.header().identifier().to_vec())
                .map_err(mlua::Error::runtime)?;
            if !name.starts_with("data.tar") {
                continue;
            }

            if name.ends_with("gz") {
                tar::Archive::new(GzDecoder::new(entry)).unpack(base)?;
            } else if name.ends_with("xz") {
                tar::Archive::new(XzDecoder::new(entry)).unpack(base)?;
            } else {
                return Err(mlua::Error::runtime("could not deturmine compression type"));
            }
            break;
        }
        self.spec.files.push(File {
            src: base.to_string_lossy().to_string(),
            dst: "/".to_string(),
            mode: None,
            dir: None,
        });
        Ok(())
    }

    fn download_binary(
        &mut self,
        url: String,
        name: Option<String>,
        opts: Option<DownloadOpts>,
    ) -> mlua::Result<()> {
        let fname = match name {
            Some(n) => n,
            None => util::url_filename(&url).map_err(|e| mlua::Error::runtime(e))?,
        };
        let out = self.bin_path(&fname);
        let opts = DownloadOpts {
            url: None,
            release: None,
            arch: None,
            out: Some(out.clone()),
            compression: opts.and_then(|o| o.compression),
        };
        dl::fetch(url, opts).map_err(|e| mlua::Error::runtime(e))?;
        self.add_bin(&out)?;
        Ok(())
    }

    fn init_build_no(&mut self) -> io::Result<()> {
        self.build = Some(self.get_build()?);
        Ok(())
    }

    fn increment_build_no(&self) -> io::Result<u32> {
        use std::io::Seek;
        let path = self.cache_dir().join("buildno.txt");
        if !path.exists() {
            let mut f = fs::File::options().write(true).create(true).open(&path)?;
            f.write(b"0")?;
            return Ok(0);
        }
        let mut f = fs::File::options().read(true).write(true).open(&path)?;
        let mut str_no = String::new();
        f.read_to_string(&mut str_no)?;
        let mut n: u32 = str_no
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        n += 1;
        f.seek(io::SeekFrom::Start(0))?;
        f.set_len(0)?;
        f.write(n.to_string().as_bytes())?;
        Ok(n)
    }

    fn get_build(&self) -> io::Result<u32> {
        let path = self.cache_dir().join("buildno.txt");
        if !path.exists() {
            let mut f = fs::File::options().write(true).create(true).open(&path)?;
            f.write(b"0")?;
            return Ok(0);
        }
        let mut f = fs::File::options().read(true).open(&path)?;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        Ok(s.trim()
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?)
    }

    fn set_build(&mut self, n: u32) -> io::Result<()> {
        let path = self.cache_dir().join("buildno.txt");
        if !path.exists() {
            let mut f = fs::File::options().write(true).create(true).open(&path)?;
            f.write(n.to_string().as_bytes())?;
            return Ok(());
        }
        let mut f = fs::File::options().write(true).truncate(true).open(&path)?;
        f.write(n.to_string().as_bytes())?;
        self.build = Some(n);
        Ok(())
    }
}

fn get_build(path: &PathBuf, f: &mut fs::File) -> io::Result<u32> {
    if !path.exists() {
        let mut newf = fs::File::options().write(true).create(true).open(&path)?;
        newf.write(b"0")?;
        return Ok(0);
    }
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s.trim()
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?)
}
