use mlua;
use std::{error, fmt, io};

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Fmt(fmt::Error),
    Lua(mlua::Error),
    Str(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Fmt(e) => e.fmt(f),
            Self::Lua(e) => e.fmt(f),
            Self::Str(s) => f.write_str(s),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.inner().source()
    }
}

impl From<mlua::Error> for Error {
    fn from(value: mlua::Error) -> Self {
        Self::Lua(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<fmt::Error> for Error {
    fn from(value: fmt::Error) -> Self {
        Self::Fmt(value)
    }
}

impl Into<mlua::prelude::LuaError> for Error {
    fn into(self) -> mlua::prelude::LuaError {
        match self {
            Self::Lua(e) => e,
            Self::Io(e) => mlua::Error::external(e.to_string()),
            Self::Str(s) => mlua::Error::external(s),
            Self::Fmt(e) => mlua::Error::external(e),
        }
    }
}

impl Into<io::Error> for Error {
    fn into(self) -> io::Error {
        match self {
            Self::Io(e) => e,
            Self::Fmt(e) => io::Error::new(io::ErrorKind::Other, e),
            Self::Lua(e) => io::Error::new(io::ErrorKind::Other, e),
            Self::Str(e) => io::Error::new(io::ErrorKind::Other, e),
        }
    }
}

impl Error {
    pub(crate) fn new(msg: &str) -> Self {
        Self::Str(msg.to_string())
    }

    fn inner(&self) -> &(dyn error::Error + 'static) {
        match self {
            Self::Io(e) => e,
            Self::Fmt(e) => e,
            Self::Lua(e) => e,
            Self::Str(_) => self,
        }
    }

    #[inline]
    pub(crate) fn to_lua(e: Self) -> mlua::Error {
        e.into()
    }
}

#[cfg(test)]
mod tests {}
