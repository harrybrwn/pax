use std::io;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::anyhow;
use anyhow::Result;
use git_url_parse::GitUrl;

#[derive(Debug, Default, pax_derive::FromLua)]
pub(crate) struct GitCloneOpts {
    pub repo: String,
    pub dest: Option<String>,
    pub branch: Option<String>,
    pub depth: Option<u32>,
    pub force: bool,
}

impl GitCloneOpts {
    pub(crate) fn new(repo: String) -> Self {
        Self {
            repo,
            dest: None,
            branch: None,
            depth: None,
            force: false,
        }
    }
}

pub(crate) fn git_clone(opts: GitCloneOpts) -> anyhow::Result<()> {
    let mut args = vec!["clone"];
    args.push(&opts.repo);
    if let Some(ref d) = opts.dest {
        args.push(d);
        if opts.force {
            _ = std::fs::remove_dir_all(d);
        }
    }
    if let Some(ref branch) = opts.branch {
        args.push("--branch");
        args.push(branch.as_str());
    }
    let mut depth_str = String::new();
    if let Some(depth) = opts.depth {
        depth_str = format!("{}", depth);
        args.push("--depth");
        args.push(&depth_str);
    } else {
        _ = depth_str;
    }
    let _code = process::Command::new("git")
        .args(args)
        .stderr(io::stderr())
        .stdout(io::stdout())
        .output()?
        .status
        .code();
    Ok(())
}

pub fn head(repo: &str) -> Result<String> {
    let r = git2::Repository::open(repo)?;
    let head = r.head()?.resolve()?.target();
    Ok(head
        .ok_or(anyhow!("failed to get git HEAD sha"))?
        .to_string())
}

#[allow(dead_code)]
pub(crate) fn clone(opts: GitCloneOpts) -> Result<()> {
    let u = GitUrl::parse(&opts.repo)
        .map_err(|e| anyhow!("could not parse url before cloning: {}", e))?;
    let mut dest = Path::new(u.name.as_str());
    if let Some(ref d) = opts.dest {
        dest = Path::new(d.as_str());
    }
    if opts.force {
        _ = std::fs::remove_dir_all(dest);
    }
    if dest.exists() {
        return Ok(());
    }

    let mut rcb = git2::RemoteCallbacks::new();
    rcb.credentials(creds_callback);
    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(rcb);
    let mut co = git2::build::CheckoutBuilder::new();
    co.remove_untracked(false);
    let mut b = git2::build::RepoBuilder::new();
    if let Some(ref branch) = opts.branch {
        b.branch(branch);
    }
    match b
        .fetch_options(fo)
        .with_checkout(co)
        .clone(&opts.repo, dest)
    {
        Err(e) => {
            println!("clone error: {:?}", e);
            return Err(e.into());
        }
        Ok(r) => {
            if let Some(ref branch) = opts.branch {
                let rf = r.find_reference(&branch)?;
                if rf.is_tag() {
                    println!("tag oid: {:?}", rf.target());
                } else if rf.is_branch() {
                    println!("branch oid: {:?}", rf.target());
                } else if rf.is_remote() {
                    println!("remote oid: {:?}", rf.target());
                } else {
                    return Err(anyhow!("could not find reference {}", branch));
                }
                // let oid = git2::Oid::from_str(&branch)?;
                // r.find_tree
                // let obj = r.find_object(oid, None)?;
                // let mut opts = CheckoutBuilder::new();
                // r.checkout_tree(&obj, Some(opts.remove_untracked(true)))?;
            }
        }
    };
    Ok(())
}

fn creds_callback(
    url: &str,
    username: Option<&str>,
    allowed: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    println!("{url} {username:?} {allowed:?}");
    if allowed.contains(git2::CredentialType::SSH_KEY) {
        let u = username.unwrap_or("git");
        println!("getting ssh key from agent");
        return git2::Cred::ssh_key_from_agent(u);
    }
    let mut cases = vec![];

    if let Some(username) = username {
        cases.push(git2::Cred::ssh_key_from_agent(username));
    }

    let sshdir: PathBuf = [env!("HOME"), ".ssh"].iter().collect();
    cases.push(git2::Cred::ssh_key(
        username.unwrap(),
        Some(sshdir.join("id_ed25519.pub").as_path()),
        sshdir.join("id_ed25519").as_path(),
        None,
    ));
    // cases.push(git2::Cred::ssh_key_from_agent(env!("USER")));
    for case in &cases {
        println!(
            "{:?}",
            case.as_ref()
                .map(|c| format!("{} {}", c.credtype(), c.has_username()))
        );
    }
    for res in cases {
        println!("maybe cred: {:?}", res.as_ref().map(|c| c.credtype()));
        match res {
            Err(e) => println!("failed to get creds: {:?}", e),
            Ok(c) => return Ok(c),
        };
    }
    println!("done searching for creds");
    Err(git2::Error::new(
        git2::ErrorCode::Auth,
        git2::ErrorClass::Ssh,
        "failed to find ssh credentials",
    ))
}
