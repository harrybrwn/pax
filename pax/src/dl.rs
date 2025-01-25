use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::{fs, str};

use anyhow::Result;
use hyper::body::Buf;

#[derive(Clone, Default, pax_derive::FromLuaTable)]
pub struct DownloadOpts {
    pub release: Option<String>,
    pub arch: Option<String>,
    pub out: Option<String>,
}

macro_rules! opt {
    ($opts:ident, $field:ident, $deflt:literal) => {
        $opts.$field.as_ref().map(|s| s.as_str()).unwrap_or($deflt)
    };
}

impl mlua::FromLua<'_> for DownloadOpts {
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

pub(crate) fn fetch(url: String, opts: DownloadOpts) -> Result<()> {
    let out = match opts
        .out
        .as_ref()
        .map(|s| s.as_str())
        .or_else(|| Some(Path::new(&url).file_name()?.to_str()?))
    {
        None => anyhow::bail!("no output file given when downloading {}", url),
        Some(s) => s,
    };
    runtime()?.block_on(download(&url, out, 0o664))?;
    Ok(())
}

pub(crate) fn kubectl(opts: DownloadOpts) -> Result<String> {
    let runtime = runtime()?;
    let mut release = opt!(opts, release, "stable").to_string();
    if release == "stable" {
        release = runtime.block_on(get_string("https://dl.k8s.io/release/stable.txt"))?;
    }
    let u = format!(
        "https://dl.k8s.io/release/{}/bin/linux/{}/kubectl",
        release,
        opt!(opts, arch, "amd64")
    );
    let out = opt!(opts, out, "bin/kubectl");
    runtime.block_on(download(&u, out, 0o755))?;
    Ok(out.into())
}

pub(crate) fn jq(opts: DownloadOpts) -> Result<String> {
    let out = opt!(opts, out, "bin/jq");
    let url = format!(
        "https://github.com/jqlang/jq/releases/download/jq-{}/jq-linux-{}",
        opt!(opts, release, "1.7.1"),
        opt!(opts, arch, "amd64")
    );
    runtime()?.block_on(download(&url, out, 0o755))?;
    Ok(out.into())
}

pub(crate) fn youtube_dl(opts: DownloadOpts) -> Result<String> {
    let url = format!(
        "https://github.com/ytdl-org/youtube-dl/releases/download/{}/youtube-dl",
        opt!(opts, release, "2021.12.17")
    );
    let out = opt!(opts, out, "bin/youtube-dl");
    runtime()?.block_on(download(&url, out, 0o755))?;
    Ok(out.into())
}

pub(crate) fn yt_dlp(opts: DownloadOpts) -> Result<String> {
    let release = opt!(opts, release, "2024.04.09");
    let out = opt!(opts, out, "bin/yt-dlp");
    let url = format!(
        "https://github.com/yt-dlp/yt-dlp/releases/download/{}/yt-dlp",
        release
    );
    runtime()?.block_on(download(&url, out, 0o755))?;
    Ok(out.into())
}

pub(crate) fn mc(opts: DownloadOpts) -> Result<String> {
    let url = format!(
        "https://dl.min.io/client/mc/release/linux-{}/mc",
        opt!(opts, arch, "amd64")
    );
    let out = opt!(opts, out, "bin/mc");
    runtime()?.block_on(download(&url, out, 0o755))?;
    Ok(out.into())
}

pub(crate) fn tetris(opts: DownloadOpts) -> Result<String> {
    let arch = match opt!(opts, arch, "x86_64") {
        "amd64" => "x86_64",
        a => a,
    };
    let url = format!(
        "https://github.com/samtay/tetris/releases/download/{}/tetris-debian-{}",
        opt!(opts, release, "0.1.4"),
        arch
    );
    let out = opt!(opts, out, "bin/tetris");
    runtime()?.block_on(download(&url, out, 0o755))?;
    Ok(out.into())
}

pub(crate) fn balena_etcher(opts: DownloadOpts) -> Result<String> {
    let arch = match opt!(opts, arch, "x64") {
        "amd64" => "x64",
        a => a,
    };
    let url = format!(
        "https://github.com/balena-io/etcher/releases/download/v{release}/balenaEtcher-{release}-{}.AppImage",
        arch,
        release=opt!(opts, release, "1.18.11")
    );
    let out = opt!(opts, out, "bin/BalenaEtcher.AppImage");
    runtime()?.block_on(download(&url, out, 0o755))?;
    Ok(out.into())
}

type Client =
    hyper::Client<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>, hyper::body::Body>;

pub fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

fn client() -> Client {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();
    hyper::Client::builder()
        .http2_max_frame_size(Some((1 << 16) - 1))
        .http2_only(false)
        .build::<_, hyper::body::Body>(https)
}

static REDIRECT_LIMIT: u8 = 10;

async fn download(u: &str, out: &str, mode: u32) -> Result<()> {
    let client = client();
    let res = get(u, client).await?;
    let body = res.into_body();
    let mut body_bytes = hyper::body::to_bytes(body).await?.reader();
    let mut f = fs::File::options()
        .mode(mode)
        .create(true)
        .write(true)
        .truncate(true)
        .open(&out)?;
    io::copy(&mut body_bytes, &mut f)?;
    Ok(())
}

async fn get_string(u: &str) -> Result<String> {
    let client = client();
    let res = get(u, client).await?;
    let body_bytes = hyper::body::to_bytes(res.into_body()).await?.to_vec();
    Ok(String::from_utf8(body_bytes)?.trim().to_string())
}

async fn get(u: &str, client: Client) -> Result<hyper::Response<hyper::Body>> {
    let mut url = String::from(u);
    let mut i = 0;
    loop {
        if i > REDIRECT_LIMIT {
            anyhow::bail!("too many redirects");
        }
        let req = hyper::Request::builder()
            .method("GET")
            .uri(url)
            .body(hyper::Body::empty())?;
        let res = client.request(req).await?;
        let status = res.status();
        if status.is_redirection() {
            if let Some(loc) = res.headers().get("location") {
                url = String::from(loc.clone().to_str()?);
                i += 1;
                continue;
            }
            anyhow::bail!("no 'location' header");
        }
        if !status.is_success() {
            anyhow::bail!("bad status code: {}", status);
        }
        break Ok(res);
    }
}
