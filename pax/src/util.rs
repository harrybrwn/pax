use std::{
    cmp::Ordering,
    fmt,
    fs::{self, DirEntry},
    hash::Hash,
    io,
    ops::Deref,
    os::raw::c_void,
    path::Path,
    process,
    str::FromStr,
};

use md5::Digest;
use mlua::{Table, Value};

use crate::error::Error;

#[inline]
pub(crate) fn get_user_name() -> std::io::Result<String> {
    git_cmd(["config", "--global", "--get", "user.name"])
}

#[inline]
pub(crate) fn get_user_email() -> std::io::Result<String> {
    git_cmd(["config", "--global", "--get", "user.email"])
}

pub(crate) fn git_version() -> std::io::Result<String> {
    git_cmd(["describe", "--tags", "HEAD"])
}

pub(crate) fn print_function<'a>(
    lua: &'a mlua::Lua,
    args: mlua::Variadic<mlua::Value>,
) -> mlua::Result<()> {
    use std::fmt::Write;
    let mut p = Printer::new(lua);
    let mut w = Writer { w: io::stdout() };
    for a in args.into_iter() {
        p.write_lua_val(&mut w, a, 0).map_err(Error::to_lua)?;
    }
    w.write_char('\n').map_err(mlua::Error::external)?;
    Ok(())
}

pub(crate) fn lua_octal(_lua: &'_ mlua::Lua, n: String) -> mlua::Result<u32> {
    Ok(u32::from_str_radix(&n, 8).map_err(mlua::Error::runtime)?)
}

pub struct Printer {
    global: *const c_void,
    package: *const c_void,
    g_rec: u32, // counts recursive prints
    p_rec: u32,
}

impl Printer {
    pub fn new(lua: &mlua::Lua) -> Self {
        let g = lua.globals();
        let pkg: mlua::Table = g.get("package").unwrap();
        Printer {
            global: g.to_pointer(),
            package: pkg.to_pointer(),
            g_rec: 0,
            p_rec: 0,
        }
    }

    fn write_lua_val<W>(&mut self, s: &mut W, val: mlua::Value, depth: usize) -> Result<(), Error>
    where
        W: fmt::Write,
    {
        match val {
            Value::Nil => write!(s, "nil"),
            Value::Boolean(v) => write!(s, "{}", v),
            Value::Integer(v) => write!(s, "{}", v),
            Value::Number(v) => write!(s, "{}", v),
            Value::String(v) => write!(s, "'{}'", v.to_string_lossy()),
            Value::Function(f) => write!(s, "<function({:?})>", f.to_pointer()),
            Value::Thread(t) => write!(s, "<thread {:?}>", t.to_pointer()),
            Value::Error(ref e) => write!(s, "{}", e),
            Value::UserData(v) => {
                let mt = v.get_metatable()?;
                if let Ok(name) = mt.get::<String>("__name") {
                    write!(s, "{}({:?})", name, v.to_pointer())
                } else {
                    write!(s, "{:?}", v)
                }
            }
            Value::LightUserData(_) => write!(s, "<lightuserdata>"),
            Value::Table(ref tab) => {
                let p = tab.to_pointer();
                if p == self.global {
                    if self.g_rec > 0 {
                        write!(s, "<globals {:?}>", p) //.map_err(mlua::Error::runtime)
                    } else {
                        self.g_rec += 1;
                        self.print_table_at_depth(s, tab, depth)
                            .map_err(mlua::Error::external)?;
                        Ok(())
                    }
                } else if p == self.package {
                    if self.p_rec > 0 {
                        write!(s, "<package {:?}>", p)
                    } else {
                        self.p_rec += 1;
                        self.print_table_at_depth(s, tab, depth)
                            .map_err(mlua::Error::external)?;
                        Ok(())
                    }
                } else {
                    self.print_table_at_depth(s, tab, depth)
                        .map_err(mlua::Error::external)?;
                    Ok(())
                }
            }
        }
        .map_err(|e| Error::from(e))
    }

    fn print_table_at_depth<'a, W: fmt::Write>(
        &mut self,
        s: &mut W,
        table: &Table<'a>,
        depth: usize,
    ) -> Result<(), Error> {
        let padding = " ".repeat((1 + depth) * 2);
        let mut pairs = table.to_owned().pairs::<Value, Value>().collect::<Vec<_>>();
        pairs.sort_by(|a, b| match (a, b) {
            (Ok((Value::String(sa), _)), Ok((Value::String(sb), _))) => sa
                .as_ref()
                .partial_cmp(sb.as_ref())
                .unwrap_or(Ordering::Equal),
            _ => Ordering::Equal,
        });
        s.write_char('{')?;
        if pairs.is_empty() {
            s.write_char('}')?;
            return Ok(());
        }
        s.write_char('\n')?;
        for pair in pairs {
            let (key, val) = pair?;
            s.write_str(&padding)?;
            match key {
                Value::String(v) => {
                    s.write_str(v.to_str()?)?;
                    s.write_str(" = ")?;
                }
                Value::Integer(_) => {} // array like table
                _ => return Err(Error::new("invalid table key, could not print")),
            };
            self.write_lua_val(s, val, depth + 1)?;
            s.write_char(',')?;
            s.write_char('\n')?;
        }
        s.write_str(&" ".repeat(depth * 2))?;
        s.write_char('}')?;
        Ok(())
    }
}

fn git_cmd<I, S>(args: I) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    use std::str; // needed for 'from_utf8'
    let out = process::Command::new("git").args(args).output()?;
    if !out.status.success() {
        let s = str::from_utf8(&out.stderr)
            .map(|s| s.strip_suffix('\n').unwrap_or(s))
            .map_err(to_io_err)?;
        return Err(to_io_err(s));
    }
    match str::from_utf8(&out.stdout)
        .map_err(to_io_err)?
        .strip_suffix('\n')
    {
        None => Err(to_io_err("no results from git command")),
        Some(s) => Ok(String::from(s)),
    }
}

pub fn to_io_err<T>(err: T) -> io::Error
where
    T: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::Other, err)
}

pub fn mtime_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// Implements both io::Write and fmt::Write
struct Writer<W> {
    w: W,
}

impl<W: io::Write> fmt::Write for Writer<W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.w.write(s.as_bytes()).map_err(|_| fmt::Error)?;
        Ok(())
    }
}

impl<W: io::Write> io::Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.w.write(buf)
    }
    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.w.write_vectored(bufs)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.w.write_all(buf)
    }
    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> io::Result<()> {
        self.w.write_fmt(fmt)
    }
}

pub(crate) struct HashReader<'a, R: io::Read, H: Digest> {
    pub r: R,
    pub h: &'a mut H,
}

impl<'a, R: io::Read, H: Digest> io::Read for HashReader<'a, R, H> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.r.read(buf)?;
        self.h.update(&buf[..n]);
        Ok(n)
    }
}

struct HashWriter<W: io::Write, H: Digest> {
    w: W,
    h: H,
}

impl<W: io::Write, H: Digest> io::Write for HashWriter<W, H> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.w.write(buf)?;
        self.h.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

macro_rules! fill_traits {
    ($t:ty, $inner:ident) => {
        impl<T> Deref for $t {
            type Target = T;
            fn deref(&self) -> &Self::Target {
                &self.$inner
            }
        }
        impl<T> AsRef<T> for FromLuaStr<T> {
            fn as_ref(&self) -> &T {
                &self.inner
            }
        }
        fill_traits!($t);
    };
    ($t:ty) => {
        impl<T: PartialEq> PartialEq for $t {
            fn eq(&self, other: &Self) -> bool {
                (**self).eq(&other.inner)
            }
        }
        impl<T: Eq> Eq for $t {}
        impl<T: PartialOrd> PartialOrd for $t {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                (**self).partial_cmp(&other.inner)
            }
        }
        impl<T: Ord> Ord for $t {
            fn cmp(&self, other: &Self) -> Ordering {
                (**self).cmp(&other.inner)
            }
        }
        impl<T: Hash> Hash for $t {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                (**self).hash(state)
            }
        }
        impl<T: Into<String>> Into<String> for $t {
            fn into(self) -> String {
                self.inner.into()
            }
        }
        impl<'a, T: Into<&'a str>> Into<&'a str> for $t {
            fn into(self) -> &'a str {
                self.inner.into()
            }
        }
    };
}

#[derive(Debug)]
pub(crate) struct FromLuaStr<T> {
    inner: T,
}

fill_traits!(FromLuaStr<T>, inner);

impl<T: ?Sized + FromStr> FromStr for FromLuaStr<T> {
    type Err = T::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            inner: T::from_str(s)?,
        })
    }
}

impl<T: ToString> ToString for FromLuaStr<T> {
    fn to_string(&self) -> String {
        (**self).to_string()
    }
}

impl<T: FromStr> mlua::FromLua<'_> for FromLuaStr<T> {
    fn from_lua(
        value: mlua::prelude::LuaValue<'_>,
        _lua: &'_ mlua::Lua,
    ) -> mlua::prelude::LuaResult<Self> {
        match value {
            mlua::Value::String(s) => Ok(Self::from_str(s.to_str()?)
                .map_err(|_| mlua::Error::runtime("failed to convert from string"))?),
            v => Err(mlua::Error::FromLuaConversionError {
                from: v.type_name(),
                to: std::any::type_name::<Self>(),
                message: None,
            }),
        }
    }
}

pub(crate) fn walk<'a, P: AsRef<Path>, F: FnMut(&DirEntry) -> io::Result<()> + 'a>(
    p: P,
    f: F,
) -> io::Result<()> {
    Walker::new(f).walk(p)
}

type WalkCallback<'a> = Box<dyn FnMut(&DirEntry) -> io::Result<()> + 'a>;

pub(crate) struct Walker<'a> {
    callback: WalkCallback<'a>,
}

impl<'a> Walker<'a> {
    fn new<F: FnMut(&DirEntry) -> io::Result<()> + 'a>(f: F) -> Self {
        Self {
            callback: Box::new(f),
        }
    }

    pub(crate) fn walk<P: AsRef<Path>>(&mut self, dir: P) -> io::Result<()> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let p = entry.path();
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    self.walk(&p)?;
                    continue;
                }
            }
            (*self.callback)(&entry)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, pax_derive::FromLua)]
pub struct SCDocOpts {
    pub input: String,
    pub output: String,
    pub compress: Option<bool>,
}

pub fn scdoc(opts: SCDocOpts) -> io::Result<()> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    let mut child = process::Command::new("scdoc")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .spawn()
        .map_err(|e| io::Error::new(e.kind(), format!("failed to spawn scdoc command: {}", e)))?;
    let stdin = child.stdin.as_mut().ok_or(io::Error::new(
        io::ErrorKind::Interrupted,
        "failed to get child process stdin",
    ))?;
    let mut infile = fs::File::open(&opts.input)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: failed to open scdoc input file", e)))?;
    io::copy(&mut infile, stdin)?;
    let out = child.wait_with_output()?;
    let mut outfile = fs::File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&opts.output)
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{}: failed to open scdoc output file {:?}", e, &opts.output),
            )
        })?;
    if opts.compress.unwrap_or(false) {
        let mut gziper = GzEncoder::new(&mut outfile, Compression::default());
        io::copy(&mut out.stdout.as_slice(), &mut gziper)?;
    } else {
        io::copy(&mut out.stdout.as_slice(), &mut outfile)?;
    }
    Ok(())
}

pub fn url_filename(input: &str) -> anyhow::Result<String> {
    let uri = url::Url::options().parse(input)?;
    Ok(uri
        .path_segments()
        .and_then(|p| p.rev().next())
        .and_then(|s| Some(String::from(s)))
        .ok_or_else(|| anyhow::anyhow!("failed to get uri path segments"))?)
}
