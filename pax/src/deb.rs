use std::{
    cmp::Ordering,
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use md5::Digest;
use mlua::{
    prelude::{LuaResult, LuaValue},
    Lua,
};

use crate::util::{mtime_now, to_io_err, walk, HashReader};
use pax_derive::UserData as PaxUserData;

pub(crate) struct DebArchive<W: Write> {
    builder: ar::Builder<W>,
    time: u64,
}

impl<W: Write> DebArchive<W> {
    pub(crate) fn new(w: W, time: u64) -> Self {
        let builder = ar::Builder::new(w);
        Self { builder, time }
    }

    pub(crate) fn init(&mut self) -> io::Result<()> {
        let mut head = ar::Header::new("debian-binary".into(), 4);
        head.set_mode(0o644);
        head.set_mtime(self.time);
        self.builder.append(&head, "2.0\n".as_bytes())
    }

    pub(crate) fn append_vec(&mut self, name: &str, data: Vec<u8>) -> io::Result<()> {
        let mut head = ar::Header::new(name.into(), data.len() as u64);
        head.set_mode(0o644);
        head.set_mtime(self.time);
        self.builder.append(&head, data.as_slice())?;
        Ok(())
    }
}

type HashPair = (md5::digest::Output<md5::Md5>, PathBuf);

pub(crate) struct DataBuilder<'a, W: Write> {
    tar: tar::Builder<W>,
    time: u64,
    size: u64,
    dirs: HashSet<PathBuf>,
    hasher: md5::Md5,
    hashes: &'a mut Vec<HashPair>,
}

impl<'a, W: Write> DataBuilder<'a, W> {
    pub fn new(w: W, hashes: &'a mut Vec<HashPair>) -> Self {
        Self {
            tar: tar::Builder::new(w),
            time: mtime_now(),
            dirs: HashSet::new(),
            hasher: md5::Md5::new(),
            hashes,
            size: 0,
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn add_path<S, D>(&mut self, source: S, dest: D) -> io::Result<()>
    where
        S: AsRef<Path>,
        D: AsRef<Path>,
    {
        let dst = strip_leading_slash(&dest);
        let stat = fs::metadata(&source).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{}: failed to stat file {:?}", e, source.as_ref()),
            )
        })?;
        let ft = stat.file_type();
        if ft.is_symlink() {
            Err(to_io_err("symlinks are not supported file types"))
        } else if ft.is_file() {
            self.add_reader_metadata(
                &dst,
                fs::File::open(&source).map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!("{}: could not open file {:?}", e, source.as_ref()),
                    )
                })?,
                stat,
            )
        } else if ft.is_dir() {
            walk(&source, |entry| {
                let path = entry.path();
                let d = dst.join(path.strip_prefix(&source).map_err(to_io_err)?);
                let meta = entry.metadata()?;
                if meta.is_symlink() {
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_mtime(meta.mtime() as u64);
                    self.tar
                        .append_link(&mut header, d, fs::read_link(&path)?)?;
                } else if meta.is_file() {
                    self.add_reader_metadata(
                        d,
                        fs::File::open(&path).map_err(|e| {
                            io::Error::new(
                                e.kind(),
                                format!("{}: could not open file {:?}", e, &path),
                            )
                        })?,
                        meta,
                    )
                    .map_err(|e| {
                        io::Error::new(
                            e.kind(),
                            format!("{}: failed to walk directory {:?}", e, source.as_ref()),
                        )
                    })?;
                }
                Ok(())
            })
        } else {
            Err(to_io_err(
                "directories and files are the only supported file types",
            ))
        }
    }

    fn add_reader<P, R>(&mut self, dest: P, reader: R, size: u64, mode: u32) -> io::Result<()>
    where
        P: AsRef<Path>,
        R: Read,
    {
        let dst = strip_leading_slash(dest);
        self.add_parent_directories(&dst)?;
        let mut head = tar::Header::new_gnu();
        head.set_mtime(self.time);
        head.set_uid(0);
        head.set_gid(0);
        head.set_mode(mode);
        head.set_size(size);
        let r = HashReader {
            r: reader,
            h: &mut self.hasher,
        };
        self.tar.append_data(&mut head, &dst, r)?;
        self.size += size;
        self.hashes.push((self.hasher.finalize_reset(), dst));
        Ok(())
    }

    #[inline]
    fn add_reader_metadata<P, R>(
        &mut self,
        dest: P,
        reader: R,
        meta: fs::Metadata,
    ) -> io::Result<()>
    where
        P: AsRef<Path>,
        R: Read,
    {
        self.add_reader(dest, reader, meta.size(), meta.mode())
    }

    fn directory(&mut self, path: &Path) -> io::Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_mtime(self.time);
        header.set_size(0);
        header.set_mode(0o755);
        let mut path_str = path.to_string_lossy().to_string();
        if !path_str.ends_with('/') {
            path_str += "/";
        }
        header.set_entry_type(tar::EntryType::Directory);
        header.set_cksum();
        self.tar
            .append_data(&mut header, path_str, &mut io::empty())
    }

    fn add_parent_directories(&mut self, path: &Path) -> io::Result<()> {
        // Append each of the directories found in the file's pathname to the archive before adding the file
        // For each directory pathname found, attempt to add it to the list of directories
        let asset_relative_dir =
            Path::new(".").join(path.parent().ok_or(to_io_err("invalid path"))?);
        let mut directory = PathBuf::new();
        for comp in asset_relative_dir.components() {
            match comp {
                //std::path::Component::CurDir => directory.push("."),
                std::path::Component::Normal(c) => directory.push(c),
                _ => continue,
            }
            if !self.dirs.contains(&directory) {
                self.dirs.insert(directory.clone());
                self.directory(&directory)?;
            }
        }
        Ok(())
    }
}

fn strip_leading_slash<P: AsRef<Path>>(path: P) -> PathBuf {
    let p = path.as_ref();
    if p.is_absolute() {
        p.iter().skip(1).collect()
    } else {
        p.to_path_buf()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PaxUserData)]
pub(crate) enum Urgency {
    Low,
    Medium,
    High,
    Emergency,
    Critical,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, pax_derive::UserDataWithDefault)]
pub(crate) enum Priority {
    Required,
    Important,
    Standard,
    #[default]
    Optional,
    Extra, // deprecated, use optional
    Invalid,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub(crate) enum Architecture {
    #[default]
    All,
    Any,
    Source,
    Invalid,
}

// TODO use /usr/share/dpkg/cputable for mapping architecture names
#[allow(dead_code)]
pub(crate) fn current_arch() -> Option<String> {
    None
}

impl From<&str> for Architecture {
    fn from(value: &str) -> Self {
        match value {
            "all" => Self::All,
            "any" => Self::Any,
            "source" => Self::Source,
            _ => Self::Invalid,
        }
    }
}

impl<'a> Into<&'a str> for Architecture {
    fn into(self) -> &'a str {
        match self {
            Self::All => "all",
            Self::Any => "any",
            Self::Source => "source",
            Self::Invalid => "<invalid>",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, pax_derive::IntoLua)]
pub struct Version {
    epoch: u32,
    major: u32,
    minor: u32,
    patch: u32,
    revision: String,
}

impl TryFrom<&str> for Version {
    type Error = io::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        // From Debian docs:
        // [epoch:]upstream_version[-debian_revision]
        // https://github.com/guillemj/dpkg/blob/main/lib/dpkg/parsehelp.c
        let mut value = value;
        if value.is_empty() {
            return Err(to_io_err("empty version value"));
        }
        let mut res = Self::default();
        if let Some((epoch, rest)) = value.split_once(':') {
            res.epoch = epoch.parse().map_err(to_io_err)?;
            value = rest;
        }
        if let Some(ix) = min(value.find('~'), min(value.find('+'), value.find('-'))) {
            res.revision.push_str(&value[ix..]);
            value = &value[..ix];
        }
        for err in value
            .strip_prefix("v")
            .unwrap_or(value)
            .split('.')
            .enumerate()
            .map(|(i, s)| {
                match s.parse() {
                    Err(e) => return Err(to_io_err(e)),
                    Ok(v) => match i {
                        0 => res.major = v,
                        1 => res.minor = v,
                        2 => res.patch = v,
                        _ => return Err(to_io_err("version has too many sections")),
                    },
                }
                Ok(())
            })
        {
            if let Err(e) = err {
                return Err(to_io_err(e));
            }
        }
        Ok(res)
    }
}

impl ToString for Version {
    fn to_string(&self) -> String {
        format!(
            "{}:{}.{}.{}{}",
            self.epoch, self.major, self.minor, self.patch, self.revision
        )
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        for n in &[
            (self.epoch, other.epoch),
            (self.major, other.major),
            (self.minor, other.minor),
            (self.patch, other.patch),
        ] {
            match n.0.cmp(&n.1) {
                Ordering::Equal => {}
                Ordering::Less => return Some(Ordering::Less),
                Ordering::Greater => return Some(Ordering::Greater),
            }
        }
        Some(self.revision.cmp(&other.revision))
    }
}

impl std::cmp::Eq for Version {}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        for n in &[
            (self.epoch, other.epoch),
            (self.major, other.major),
            (self.minor, other.minor),
            (self.patch, other.patch),
        ] {
            match n.0.cmp(&n.1) {
                Ordering::Equal => {}
                Ordering::Less => return Ordering::Less,
                Ordering::Greater => return Ordering::Greater,
            }
        }
        self.revision.cmp(&other.revision)
    }
}

impl Version {
    fn new_full<S: AsRef<str>>(epoch: u32, major: u32, minor: u32, patch: u32, rev: S) -> Self {
        Self {
            epoch,
            major,
            minor,
            patch,
            revision: rev.as_ref().to_string(),
        }
    }

    fn new_basic(major: u32, minor: u32, patch: u32) -> Self {
        Self::new_full(0, major, minor, patch, "")
    }
}

impl mlua::FromLua<'_> for Version {
    fn from_lua(value: LuaValue<'_>, _lua: &'_ Lua) -> LuaResult<Self> {
        use mlua::Value;
        match value {
            Value::Table(t) => Ok(Self {
                epoch: t.get("epoch").map_err(|e| match &e {
                    mlua::Error::FromLuaConversionError { from, to, message } => {
                        mlua::Error::FromLuaConversionError {
                            from,
                            to,
                            message: Some(match *from {
                                "nil" => match message {
                                    None => format!("error at field {:?}", "epoch"),
                                    Some(msg) => format!("error at field {:?}: {}", "epoch", msg),
                                },
                                _ => match message {
                                    None => format!("error at field {:?}", "epoch"),
                                    Some(msg) => format!("error at field {:?}: {}", "epoch", msg),
                                },
                            }),
                        }
                    }
                    _ => e,
                })?,
                major: t.get("major")?,
                minor: t.get("minor")?,
                patch: t.get("patch")?,
                revision: t.get("revision")?,
            }),
            Value::String(s) => Self::try_from(s),
            Value::Integer(n) => Ok(Self::new_basic(n as u32, 0, 0)),
            Value::Number(n) => Ok(Self::new_basic(n as u32, 0, 0)),
            Value::Nil => Ok(Self::default()),
            Value::Function(_) => Err(mlua::Error::FromLuaConversionError {
                from: "function",
                to: "Version",
                message: Some("versions must be a string or table".into()),
            }),
            _ => Err(mlua::Error::runtime("failed to parse version: wrong type")),
        }
    }
}

impl TryFrom<String> for Version {
    type Error = io::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl TryFrom<mlua::String<'_>> for Version {
    type Error = mlua::Error;
    fn try_from(value: mlua::String) -> Result<Self, Self::Error> {
        Ok(Self::try_from(value.to_str()?)?)
    }
}

fn min<T>(a: Option<T>, b: Option<T>) -> Option<T>
where
    T: Ord,
{
    match (a, b) {
        (Some(aa), Some(bb)) => Some(std::cmp::min(aa, bb)),
        (Some(aa), None) => Some(aa),
        (None, Some(bb)) => Some(bb),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{DataBuilder, Version};
    use std::io::Write;

    #[test]
    fn data_builder() {
        let mut buf = Vec::<u8>::new();
        let mut hashes = Vec::new();
        (|| {
            let mut b = DataBuilder::new(&mut buf, &mut hashes);
            b.add_path("test/d", "/usr/share/d")?;
            b.add_path("test/one", "/usr/share/one")?;
            b.add_path("test/two", "/usr/share/two")?;
            Ok::<_, std::io::Error>(())
        })()
        .unwrap();
        assert!(buf.len() > 1024, "should write more than just the header");
        assert_eq!(hashes.len(), 7);
        let expected = [
            (
                [
                    32, 200, 96, 93, 190, 211, 21, 43, 15, 163, 8, 46, 157, 123, 5, 86,
                ],
                "usr/share/d/3.txt",
            ),
            (
                [
                    125, 13, 132, 166, 200, 74, 4, 9, 74, 121, 71, 158, 5, 44, 158, 165,
                ],
                "usr/share/d/2.txt",
            ),
            (
                [
                    111, 133, 147, 154, 130, 34, 211, 16, 129, 18, 127, 170, 116, 164, 37, 1,
                ],
                "usr/share/d/4.txt",
            ),
            (
                [
                    96, 128, 198, 160, 231, 134, 243, 130, 209, 113, 177, 209, 163, 155, 144, 251,
                ],
                "usr/share/d/5.txt",
            ),
            (
                [
                    172, 125, 166, 55, 230, 169, 176, 205, 167, 254, 43, 161, 4, 62, 96, 254,
                ],
                "usr/share/d/1.txt",
            ),
            (
                [
                    50, 137, 115, 144, 154, 102, 167, 75, 213, 238, 87, 209, 184, 172, 224, 14,
                ],
                "usr/share/one",
            ),
            (
                [
                    222, 138, 9, 95, 87, 109, 3, 224, 205, 33, 68, 83, 165, 121, 70, 177,
                ],
                "usr/share/two",
            ),
        ];
        for (i, exp) in expected.iter().enumerate() {
            assert_eq!(hashes[i], (exp.0.into(), exp.1.into()));
        }
        let mut a = tar::Archive::new(&buf[..]);
        let mut entries = a.entries().unwrap().into_iter().enumerate();
        let expected = [
            "usr/",
            "usr/share/",
            "usr/share/d/",
            "usr/share/d/3.txt",
            "usr/share/d/2.txt",
            "usr/share/d/4.txt",
            "usr/share/d/5.txt",
            "usr/share/d/1.txt",
            "usr/share/one",
            "usr/share/two",
        ];
        while let Some((i, Ok(e))) = entries.next() {
            let h = e.header();
            assert_eq!(expected[i], h.path().unwrap().to_str().unwrap());
            // println!("{:?}", h);
        }
    }

    #[test]
    fn get_debian_data() {
        let mut s = String::new();
        use std::io::Read;
        std::fs::File::options()
            .read(true)
            .write(false)
            .open("/usr/share/dpkg/cputable")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        println!("{}", s);
    }

    #[test]
    fn parse_invalid_version() {
        for tt in [
            "",
            "1:-ubuntu1.0",
            "a",
            "A:1.2.3",
            "2:7.4.!052-1ubuntu3.1",
            "2:7.4!052-1ubuntu3.1",
            "1.1.1.1.1.1",
        ] {
            match Version::try_from(tt) {
                Ok(_) => panic!("should not be able to parse version string {:?}", tt),
                Err(_) => {}
            };
        }
    }

    #[allow(unreachable_code)]
    #[test]
    fn version() {
        struct TT {
            input: &'static str,
            out: Version,
        }
        impl TT {
            fn new(i: &'static str, out: Version) -> Self {
                Self { input: i, out }
            }
        }

        for tt in &[
            TT::new("v71.2.13", Version::new_basic(71, 2, 13)),
            TT::new("3.2.1", Version::new_basic(3, 2, 1)),
            TT::new("4:3.2.1", Version::new_full(4, 3, 2, 1, "")),
            TT::new("1.22-1", Version::new_full(0, 1, 22, 0, "-1")),
            TT::new("10", Version::new_basic(10, 0, 0)),
            TT::new("5:v1.9", Version::new_full(5, 1, 9, 0, "")),
            TT::new(
                "9:1.51.8~20.04.1+1.4-0ubuntu0.1",
                Version::new_full(9, 1, 51, 8, "~20.04.1+1.4-0ubuntu0.1"),
            ),
            TT::new(
                "2:7.3.429-2ubuntu2.1",
                Version::new_full(2, 7, 3, 429, "-2ubuntu2.1"),
            ),
            TT::new(
                "6.1.0-0+maxmind1~focal",
                Version::new_full(0, 6, 1, 0, "-0+maxmind1~focal"),
            ),
            TT::new(
                "2:102.11+LibO6.4.7-0ubuntu0.20.04.9",
                Version::new_full(2, 102, 11, 0, "+LibO6.4.7-0ubuntu0.20.04.9"),
            ),
        ] {
            //println!("try_from({:?})", tt.input);
            let v = Version::try_from(tt.input).unwrap();
            assert_eq!(v, tt.out);
        }
        // [epoch:]upstream_version[-debian_revision]
        //
        // 2:7.3.429-2ubuntu2.1
        // 1.11-1
        // 1.13.4-2ubuntu1
        // 1.0.25+dfsg-0ubuntu5
        // 1:20190410+repack1-2
        // 2:102.11+LibO6.4.7-0ubuntu0.20.04.9
        // 1.51.1~20.04.1+1.4-0ubuntu0.1
        // 6.1.0-0+maxmind1~focal
        // 1:233-1
        // 10
        // 3:6.04~git20190206.bf6db5b4+dfsg1-2

        return;
        dpkg_compare_versions("1:2-1", ">=", "1:2-2");
        dpkg_compare_versions("2.5.3+dfsg-4", ">=", "2.5.3-dfsg-4");
        dpkg_compare_versions("2.5.3-dfsg-4", ">=", "2.5.3+dfsg-4");
        dpkg_compare_versions("6.1.0-0+maxmind1~focal", "<<", "6.1.0-0+maxmind1~focal");
        dpkg_compare_versions("2:7.4.!052-1ubuntu3.1", "<=", "2:7.4-052-1ubuntu3.1");
    }

    fn dpkg_compare_versions(a: &str, op: &str, b: &str) -> bool {
        use std::process;
        let res = match process::Command::new("dpkg")
            .args(&["--compare-versions", a, op, b])
            .output()
        {
            Err(e) => {
                println!("Error: {:?}", e);
                false
            }
            Ok(out) => {
                std::io::stdout().write_all(&out.stdout).unwrap();
                std::io::stderr().write_all(&out.stderr).unwrap();
                // println!("status: {}", out.status);
                out.status.success()
            }
        };
        println!("dpkg_compare_versions:");
        println!("  ({:?}, {:?}, {:?}) => {}", a, op, b, res);
        println!("  {:?}.cmp({:?}) => {:?}", a, b, a.cmp(b));
        println!();
        res
    }
}
