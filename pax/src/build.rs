use std::{
    cell::{Ref, RefCell, RefMut},
    fs,
    io::{self, BufRead, BufWriter, Write},
    path::{Path, PathBuf},
};

use md5::{Digest, Md5};
use mlua::{
    prelude::{LuaResult, LuaValue},
    Lua,
};
use pax_derive;
use pax_derive::{FromLua, IntoLua as PaxIntoLua};

use crate::deb::{self, MaintainerScripts};
use crate::util::{mtime_now, to_io_err};

pub(crate) static DEFAULT_DIST: &str = "dist";

#[derive(Default, Debug, Clone, FromLua, PaxIntoLua)]
pub(crate) struct BuildSpec {
    pub(crate) package: String,
    pub(crate) name: Option<String>,
    pub(crate) version: String,
    pub(crate) description: Option<String>,
    pub(crate) essential: bool,
    pub(crate) author: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) maintainer: Option<String>,
    pub(crate) homepage: Option<String>,
    #[lua_default(vec![])]
    pub(crate) files: Vec<File>,
    #[lua_default(vec![])]
    pub(crate) dependencies: Vec<String>,
    pub(crate) recommends: Option<Vec<String>>,
    pub(crate) suggests: Option<Vec<String>>,
    pub(crate) priority: deb::Priority,
    #[lua_default("all".to_string())]
    pub(crate) arch: String,
    pub(crate) urgency: Option<deb::Urgency>,
    pub(crate) section: Option<String>,
    pub(crate) apt_sources: Option<Vec<AptSources>>,
    pub(crate) scripts: Option<MaintainerScripts>,

    #[ignored]
    pub(crate) buildno: Option<u32>,
}

macro_rules! tar_header {
    ($path:expr, $mtime:expr, $size:expr) => {
        tar_header!($path, $mtime, 0o644, $size)
    };
    ($path:expr, $mtime:expr, $mode:expr, $size:expr) => {{
        let mut head = ::tar::Header::new_gnu();
        head.set_path($path)?;
        head.set_mtime($mtime);
        head.set_uid(0);
        head.set_gid(0);
        head.set_mode($mode);
        head.set_size($size as u64);
        if let Some(ustar) = head.as_ustar_mut() {
            ustar.set_device_major(0);
            ustar.set_device_minor(0);
        }
        if let Some(gnu) = head.as_gnu_mut() {
            gnu.set_device_major(0);
            gnu.set_device_minor(0);
        }
        head.set_cksum();
        head
    }};
}

// MD5 binary hash length
const MD5_LEN: usize = 16;

struct DataMetadata {
    // vec of (hash, filename)
    hashes: Vec<(md5::digest::Output<Md5>, PathBuf)>,
    size: u64,
}

impl BuildSpec {
    pub(crate) fn generate_control<W>(&self, w: &mut W, install_size: u64) -> io::Result<()>
    where
        W: io::Write,
    {
        writeln!(w, "Package: {}", self.package)?;
        writeln!(w, "Version: {}", self.version())?;
        let priority: &str = self.priority.into();
        if let Some(ref section) = self.section {
            writeln!(w, "Section: {}", section)?;
        } else {
            writeln!(w, "Section: misc")?;
        }
        writeln!(w, "Priority: {}", priority)?;
        writeln!(w, "Architecture: {}", self.arch)?;
        if let Some(maintainer) = &self.maintainer {
            writeln!(w, "Maintainer: {}", maintainer)?;
        } else {
            match (&self.author, &self.email) {
                (Some(author), Some(email)) => writeln!(w, "Maintainer: {} <{}>", author, email)?,
                (Some(author), None) => writeln!(w, "Maintainer: {}", author)?,
                (None, Some(email)) => writeln!(w, "Maintainer: {}", email)?,
                (None, None) => {
                    return Err(to_io_err("could not build maintainer"));
                }
            };
        }

        if let Some(urgency) = self.urgency {
            let s: &str = urgency.into();
            writeln!(w, "Urgency: {}", s)?;
        }
        if install_size > 0 {
            let size = (install_size as f64) / 1024.0;
            writeln!(w, "Installed-Size: {}", size.ceil() as u64)?;
        }
        if let Some(ref homepage) = self.homepage {
            writeln!(w, "Homepage: {}", homepage)?;
        }
        if self.essential {
            writeln!(w, "Essential: yes")?;
        }
        if !self.dependencies.is_empty() {
            writeln!(w, "Depends: {}", self.dependencies.join(", "))?;
        }
        if let Some(desc) = &self.description {
            writeln!(w, "Description: {}", desc)?;
        }
        if let Some(recommends) = &self.recommends {
            if !recommends.is_empty() {
                writeln!(w, "Recommends: {}", recommends.join(", "))?;
            }
        }
        if let Some(suggests) = &self.suggests {
            if !suggests.is_empty() {
                writeln!(w, "Suggests: {}", suggests.join(", "))?;
            }
        }
        Ok(())
    }

    pub(crate) fn pre_process(&mut self, base: Option<String>) -> io::Result<()> {
        if let Some(base) = base {
            let dst = Path::new(&base);
            for file in &mut self.files {
                if file.dst.is_empty() {
                    if let Some(filename) = Path::new(&file.src).file_name() {
                        if let Some(dst) = dst.join(filename).to_str() {
                            file.dst = String::from(dst);
                        }
                    }
                }
            }
        } else {
            for file in &mut self.files {
                if file.dst.is_empty() {
                    file.dst = file.src.clone();
                }
            }
        }
        Ok(())
    }

    pub(crate) fn build<P>(&mut self, dir: P) -> io::Result<()>
    where
        P: AsRef<std::path::Path>,
    {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::os::unix::fs::OpenOptionsExt; // adds .mode() to File::options

        self.validate()?;
        let path = dir.as_ref().join(self.filename());
        let package_file = fs::File::options()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o666)
            .open(path)?;
        let now = mtime_now();
        let mut archive = deb::DebArchive::new(BufWriter::new(package_file), now);
        archive.init()?;

        let mut ctrl_buf = vec![];
        let mut data_buf = vec![];
        let ctrl_enc = GzEncoder::new(&mut ctrl_buf, Compression::default());
        let data_enc = GzEncoder::new(&mut data_buf, Compression::default());

        let mut hashes = Vec::with_capacity(self.files.len());
        let size = {
            let mut b = deb::DataBuilder::new(data_enc, &mut hashes);
            let files = &mut self.files;
            files.sort_by_key(|f| f.dst.clone());
            for file in files {
                if let Some(ref dir) = file.dir {
                    b.add_dir(dir, file.mode.unwrap_or(0o755))?;
                } else if file.src.len() == 0 {
                    b.add_dir(&file.dst, file.mode.unwrap_or(0o755))?;
                } else {
                    b.add_path(&file.src, &file.dst, file.mode).map_err(|e| {
                        io::Error::new(e.kind(), format!("{}: failed to add file to archive", e))
                    })?;
                }
            }
            b.size()
        };
        self.control_tarball(ctrl_enc, DataMetadata { size, hashes })?;
        // The order that these are inserted into the archive matters. The wrong order will break
        // the installation.
        archive.append_vec("control.tar.gz", ctrl_buf)?;
        archive.append_vec("data.tar.gz", data_buf)?;
        Ok(())
    }

    fn control_tarball<W: io::Write>(&self, w: W, data: DataMetadata) -> io::Result<()> {
        let now = mtime_now();
        let mut ball = tar::Builder::new(w);
        let mut control_buf: Vec<u8> = vec![];
        self.generate_control(&mut control_buf, data.size)?;
        ball.append(
            &tar_header!("control", now, control_buf.len()),
            control_buf.as_slice(),
        )?;
        let mut md5sum_buf: Vec<u8> =
            Vec::with_capacity(self.files.len() * (Md5::output_size() + 2));
        let mut hex_buf: [u8; MD5_LEN * 2] = [0; MD5_LEN * 2];
        for hash in data.hashes {
            hex::encode_to_slice(hash.0, &mut hex_buf).map_err(to_io_err)?;
            md5sum_buf.write(&hex_buf)?;
            md5sum_buf.write(&[' ' as u8, ' ' as u8])?;
            md5sum_buf.write(hash.1.to_string_lossy().as_bytes())?;
            md5sum_buf.push('\n' as u8);
            zero(&mut hex_buf);
        }
        ball.append(
            &tar_header!("md5sums", now, md5sum_buf.len()),
            md5sum_buf.as_slice(),
        )?;

        if let Some(ref sources) = self.apt_sources {
            let mut preinst = Vec::new();
            let mut postrm = Vec::new();
            preinst.write("#!/bin/sh\nset -eu\nmkdir -p /usr/share/keyrings/\n".as_bytes())?;
            postrm.write("#!/bin/sh\nset -eu\n".as_bytes())?;
            for s in sources {
                preinst.write(
                    format!(
                        "sudo wget -q -O '/usr/share/keyrings/{name}.gpg' '{}'\n\
                        sudo chmod a+r /usr/share/keyrings/{name}.gpg\n",
                        s.gpg_key_url,
                        name = s.name,
                    )
                    .as_bytes(),
                )?;
                preinst.write(
                    format!(
                        "echo \"deb [signed-by=/usr/share/keyrings/{name}.gpg arch=$(dpkg --print-architecture)] \
                        {} {}\" | sudo tee /etc/apt/sources.list.d/{name}.list\n", s.url, s.components, name=s.name)
                    .as_bytes(),
                )?;
                postrm.write(format!("rm -f /usr/share/keyrings/{name}.gpg /etc/apt/sources.list.d/{name}.list\n", name=s.name).as_bytes())?;
            }
            ball.append(
                &tar_header!("preinst", now, 0o755, preinst.len()),
                preinst.as_slice(),
            )?;
            ball.append(
                &tar_header!("postrm", now, 0o755, postrm.len()),
                postrm.as_slice(),
            )?;
        } else if let Some(ref scripts) = self.scripts {
            if let Some(ref preinst) = scripts.preinst {
                ball.append(
                    &tar_header!("preinst", now, 0o755, preinst.len()),
                    preinst.trim().as_bytes(),
                )?;
            }
            if let Some(ref postinst) = scripts.postinst {
                ball.append(
                    &tar_header!("postinst", now, 0o755, postinst.len()),
                    postinst.trim().as_bytes(),
                )?;
            }
            if let Some(ref prerm) = scripts.prerm {
                ball.append(
                    &tar_header!("prerm", now, 0o755, prerm.len()),
                    prerm.trim().as_bytes(),
                )?;
            }
            if let Some(ref postrm) = scripts.postrm {
                ball.append(
                    &tar_header!("postrm", now, 0o755, postrm.len()),
                    postrm.trim().as_bytes(),
                )?;
            }
        }
        Ok(())
    }

    fn filename(&self) -> String {
        format!("{}-v{}_{}.deb", self.package, self.version(), self.arch)
    }

    fn version(&self) -> String {
        match self.buildno {
            Some(n) if n > 0 => format!("{}-{}", self.version, n),
            _ => self.version.clone(),
        }
    }

    fn validate(&self) -> io::Result<()> {
        if self.maintainer.is_none() && self.author.is_none() && self.email.is_none() {
            return Err(to_io_err(
                "need author and email to infer Maintainer attribute",
            ));
        }
        Ok(())
    }

    pub(crate) fn merge_in(&mut self, src: &Self) {
        if self.email.is_none() {
            self.email = src.email.clone();
        }
        if self.author.is_none() {
            self.author = src.author.clone();
        }
    }

    pub(crate) fn from_toml_with_overrides(
        conf: toml::Table,
        overrides: mlua::Table,
    ) -> mlua::Result<Self> {
        let pkg = &conf["package"];
        let package = toml_get_str(pkg, "name")?;
        let version = toml_get_str(pkg, "version").or_else(|_| overrides.get("version"))?;
        let homepage = toml_get_str(pkg, "homepage")
            .or_else(|_| overrides.get("homepage"))
            .map_or(None, Some);
        let description = toml_get_str(pkg, "description")
            .or_else(|_| overrides.get("description"))
            .map_or(None, Some);
        let mut files: Vec<File> = overrides.get("files").unwrap_or(Vec::new());
        files.push(File::from_paths(
            ["target", "release", &package].iter().collect(),
            Path::new("/usr/bin").join(&package),
            0o775,
        )?);
        let maintainer = match pkg.get("authors") {
            Some(toml::Value::Array(authors)) if authors.len() > 0 && authors[0].is_str() => {
                authors[0].as_str().map(|s| String::from(s))
            }
            _ => None,
        };
        macro_rules! fill_from {
            ($table:ident, $key:literal, $deflt:expr) => {
                match $table.get($key) {
                    Ok(v) => v,
                    Err(mlua::Error::FromLuaConversionError { from, .. }) if from == "nil" => {
                        $deflt
                    }
                    Err(e) => return Err(e),
                }
            };
        }
        Ok(Self {
            name: Some(package.clone()),
            package,
            version,
            files,
            description,
            arch: fill_from!(overrides, "arch", "all".to_string()),
            homepage,
            maintainer,
            author: overrides.get("author")?,
            email: overrides.get("email")?,
            essential: overrides.get("essential")?,
            dependencies: fill_from!(overrides, "dependencies", Vec::new()),
            recommends: fill_from!(overrides, "recommends", None),
            suggests: fill_from!(overrides, "suggests", None),
            priority: overrides.get("priority")?,
            urgency: overrides.get("urgency")?,
            section: overrides.get("section")?,
            apt_sources: overrides.get("apt_sources")?,
            scripts: overrides.get("scripts").ok(),
            buildno: None,
        })
    }

    pub(crate) fn parse<R: io::Read>(r: R) -> anyhow::Result<Self> {
        let mut s = Self::default();
        let buf = io::BufReader::new(r);
        for line in buf.lines().map_while(Result::ok) {
            if let Some(ix) = line.find(':') {
                let (key, val) = line.split_at(ix);
                match key.trim().to_lowercase().as_ref() {
                    "package" => s.package = String::from(val.trim()),
                    "version" => s.version = String::from(val.trim()),
                    "maintainer" => s.maintainer = Some(String::from(val.trim())),
                    // "urgency" => s.urgency = Some(val.trim().into()),
                    "homepage" => s.homepage = Some(String::from(val.trim())),
                    _ => {}
                };
            }
        }
        todo!()
    }
}

fn zero<T: Default, const N: usize>(arr: &mut [T; N]) {
    for i in 0..N {
        arr[i] = T::default();
    }
}

pub(crate) struct RefCellBuildSpec(pub(crate) RefCell<BuildSpec>);

impl RefCellBuildSpec {
    #[inline]
    fn new(spec: BuildSpec) -> Self {
        Self(RefCell::new(spec))
    }
    #[inline]
    pub(crate) fn borrow_mut(&self) -> RefMut<BuildSpec> {
        self.0.borrow_mut()
    }
    #[allow(dead_code)]
    #[inline]
    pub(crate) fn borrow(&self) -> Ref<BuildSpec> {
        self.0.borrow()
    }
    #[inline]
    pub(crate) fn take(&self) -> BuildSpec {
        self.0.take()
    }
}

impl mlua::FromLua<'_> for RefCellBuildSpec {
    fn from_lua(value: LuaValue<'_>, lua: &'_ Lua) -> LuaResult<Self> {
        Ok(Self::new(BuildSpec::from_lua(value, lua)?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PaxIntoLua)]
pub(crate) struct File {
    pub src: String,
    pub dst: String,
    pub mode: Option<u32>,
    pub dir: Option<String>,
}

impl File {
    pub fn new<S: AsRef<str>>(src: S, dst: S) -> Self {
        Self {
            src: String::from(src.as_ref()),
            dst: String::from(dst.as_ref()),
            mode: None,
            dir: None,
        }
    }

    pub fn from_paths<P>(src: P, dst: P, mode: u32) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        let src = match src.as_ref().to_str() {
            Some(s) => String::from(s),
            None => return Err(to_io_err("failed to convert path to string")),
        };
        let dst = match dst.as_ref().to_str() {
            Some(s) => String::from(s),
            None => return Err(to_io_err("failed to convert path to string")),
        };
        Ok(Self {
            src,
            dst,
            mode: Some(mode),
            dir: None,
        })
    }
}

impl mlua::FromLua<'_> for File {
    fn from_lua(value: LuaValue<'_>, _lua: &'_ Lua) -> LuaResult<Self> {
        use mlua::Value as V;
        match value {
            V::Table(tbl) => Ok(Self {
                src: tbl.get("src")?,
                dst: tbl.get("dst")?,
                mode: tbl.get("mode").ok(),
                dir: None,
            }),
            V::String(src) => {
                let s = src.to_str()?;
                if let Some((src, dst)) = s.split_once(':') {
                    Ok(Self::new(src, dst))
                } else {
                    Ok(Self::new(s, ""))
                }
            }
            _ => Err(mlua::Error::runtime("invalid file location")),
        }
    }
}

impl<T: AsRef<Path>> TryFrom<(T, T)> for File {
    type Error = io::Error;
    fn try_from(value: (T, T)) -> Result<Self, Self::Error> {
        Self::try_from((value.0, value.1, 0o644))
    }
}

impl<T: AsRef<Path>> TryFrom<(T, T, u32)> for File {
    type Error = io::Error;
    fn try_from(value: (T, T, u32)) -> Result<Self, Self::Error> {
        let src = match value.0.as_ref().to_str() {
            None => return Err(to_io_err("failed to convert path to string")),
            Some(s) => String::from(s),
        };
        let dst = match value.1.as_ref().to_str() {
            Some(s) => String::from(s),
            None => return Err(to_io_err("failed to convert path to string")),
        };
        Ok(Self {
            src,
            dst,
            mode: Some(value.2),
            dir: None,
        })
    }
}

#[derive(Debug, Default, PartialEq)]
struct Arch {
    os: String,
    vendor: String,
    arch: String,
}

#[allow(dead_code)]
impl Arch {
    pub(crate) fn new<S: AsRef<str>>(os: S, vendor: S, arch: S) -> Self {
        Self {
            os: String::from(os.as_ref()),
            vendor: String::from(vendor.as_ref()),
            arch: String::from(arch.as_ref()),
        }
    }

    fn to_string(&self) -> String {
        match (
            self.vendor.is_empty(),
            self.os.is_empty(),
            self.arch.is_empty(),
        ) {
            (false, false, false) => format!("{}-{}-{}", self.vendor, self.os, self.arch),
            (true, false, false) => format!("{}-{}", self.os, self.arch),
            (true, true, false) => self.arch.clone(),
            _ => String::from("<invalid architecture>"),
        }
    }

    fn from_str<S>(s: S) -> Self
    where
        S: AsRef<str>,
    {
        let parts = s.as_ref().split('-').collect::<Vec<&str>>();
        match parts.len() {
            1 => Self {
                arch: String::from(parts[0]),
                os: "".to_string(),
                vendor: "".to_string(),
            },
            2 => Self {
                os: String::from(parts[0]),
                arch: String::from(parts[1]),
                vendor: "".to_string(),
            },
            3 => Self {
                vendor: String::from(parts[0]),
                os: String::from(parts[1]),
                arch: String::from(parts[2]),
            },
            _ => Self::default(),
        }
    }
}

impl From<&str> for Arch {
    fn from(value: &str) -> Self {
        Self::from_str(value)
    }
}

impl From<String> for Arch {
    fn from(value: String) -> Self {
        Self::from_str(&value)
    }
}

impl mlua::FromLua<'_> for Arch {
    fn from_lua(value: LuaValue<'_>, _lua: &'_ Lua) -> LuaResult<Self> {
        match value {
            LuaValue::String(string) => {
                let s = string.to_string_lossy();
                Ok(Self::from_str(s))
            }
            _ => Err(mlua::Error::runtime("invalid architecture type")),
        }
    }
}

impl mlua::IntoLua<'_> for Arch {
    fn into_lua(self, _lua: &'_ Lua) -> LuaResult<LuaValue<'_>> {
        Err(mlua::Error::ToLuaConversionError {
            from: "Arch",
            to: "table",
            message: None,
        })
    }
}

#[derive(Clone, Debug, pax_derive::IntoLua, pax_derive::FromLua)]
pub(crate) struct AptSources {
    name: String,
    url: String,
    components: String,
    gpg_key_url: String,
}

fn toml_get_str(t: &toml::Value, key: &str) -> io::Result<String> {
    match t.get(key) {
        None => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "could not find value",
        )),
        Some(v) => match v {
            toml::Value::String(s) => Ok(s.to_owned()),
            _ => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "could not find value",
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{self, Read},
        path::PathBuf,
    };

    use super::Arch;

    #[test]
    fn arch() {
        struct TT {
            input: &'static str,
            out: Arch,
        }
        for tt in &[
            TT {
                input: "amd64",
                out: Arch::new("", "", "amd64"),
            },
            TT {
                input: "freebsd-i386",
                out: Arch::new("freebsd", "", "i386"),
            },
            TT {
                input: "armhf",
                out: Arch::new("", "", "armhf"),
            },
            TT {
                input: "musl-linux-powerpc",
                out: Arch::new("linux", "musl", "powerpc"),
            },
        ] {
            let a = Arch::from(tt.input);
            assert_eq!(a, tt.out);
        }
    }

    #[test]
    fn increment_file_number() {
        use std::io::{Seek, SeekFrom, Write};
        let p: PathBuf = ["/tmp", "number.txt"].iter().collect();
        _ = fs::remove_file(&p);
        {
            let mut f = fs::File::options()
                .write(true)
                .create(true)
                .open(&p)
                .unwrap();
            f.write(b"0").unwrap();
        }
        {
            let mut f = fs::File::options().read(true).write(true).open(&p).unwrap();
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            let mut n: u32 = s.trim().parse().unwrap();
            assert_eq!(n, 0);
            n += 2;
            f.seek(SeekFrom::Start(0)).unwrap();
            f.set_len(0).unwrap();
            f.write(n.to_string().as_bytes()).unwrap();
        }
        {
            let mut f = fs::File::options().read(true).open(&p).unwrap();
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            let n: u32 = s.parse().unwrap();
            assert_eq!(n, 2);
        }
        _ = fs::remove_file(&p);
    }
}
