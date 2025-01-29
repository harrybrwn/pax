use std::path::PathBuf;

use anyhow::anyhow;
use cargo::core::compiler::{CompileKind, CompileMode, CompileTarget};
use cargo::core::resolver::CliFeatures;
use cargo::core::{
    Features, Shell, VirtualManifest, Workspace, WorkspaceConfig, WorkspaceRootConfig,
};
use cargo::ops::CompileOptions;
use cargo::ops::{self, CompileFilter};
use cargo::util::command_prelude::root_manifest;
use cargo::util::{homedir, interning::InternedString};
use cargo::GlobalContext;

#[derive(Debug, pax_derive::FromLua)]
pub(crate) struct Cargo {
    pub root: String,
    pub pkgid: Option<String>,
    pub target_dir: Option<String>,
    /// profile can be "release" or "debug" and corresponds with cargo's --profile and --release
    /// flags. Default is "release".
    pub profile: Option<String>,
    /// verbosity level. 0 for off, 1 for on, 2 for very verbose
    pub verbosity: Option<u32>,
    pub features: Option<Vec<String>>,
    /// run cargo quietly.
    pub quiet: bool,
    /// Don't stop the build on failure.
    pub keep_going: bool,
    pub ignore_rust_version: bool,
    /// key value pairs that are equivilent to using --config <KEY=VAL> in the cargo cli.
    pub config: Option<Vec<String>>,
    pub target: Option<String>,

    /// run cargo as an embedded library (it doesn't always work as expected)
    pub embeded_cargo: bool,
    /// remove the target directory before building.
    pub clean: bool,
}

impl Cargo {
    pub(crate) fn build(&self) -> anyhow::Result<()> {
        if !self.embeded_cargo {
            return self.run_from_shell();
        }
        let cwd = self.root();
        let mut config = GlobalContext::new(
            Shell::new(),
            cwd.clone(),
            homedir(&cwd).ok_or_else(|| {
                anyhow!(
                    "Cargo couldn't find your home directory. \
                 This probably means that $HOME was not set."
                )
            })?,
        );
        let cli_config = self.config.to_owned().unwrap_or(vec![]);

        config.configure(
            self.verbosity.unwrap_or(0),
            self.quiet,
            None,
            false,
            false,
            false,
            &self.target_dir.as_ref().map(PathBuf::from),
            &[],
            &cli_config,
        )?;
        let manifest = root_manifest(None, &config)?;
        let mut ws = Workspace::new(&manifest, &config)?;
        ws.set_require_optional_deps(true);
        if self.clean {
            std::fs::remove_dir_all(ws.target_dir().as_path_unlocked())?;
        }

        let mut options = CompileOptions::new(&config, CompileMode::Build)?;
        options.build_config.requested_profile = self.profile();
        options.build_config.keep_going = self.keep_going;
        options.build_config.unit_graph = false;
        options.honor_rust_version = Some(!self.ignore_rust_version);
        if let Some(ref pkgid) = self.pkgid {
            options.filter = CompileFilter::single_bin(pkgid.clone());
            options.spec = ops::Packages::Packages(vec![pkgid.clone()]);
        } else {
            options.spec = ops::Packages::Default;
        }
        if let Some(ref target) = self.target {
            let kind = CompileKind::Target(CompileTarget::new(target)?);
            options.build_config.requested_kinds = vec![kind];
        }
        if let Some(ref features) = self.features {
            options.cli_features = CliFeatures::from_command_line(features, false, true)?;
        }
        ops::compile(&ws, &options)?;
        Ok(())
    }

    pub(crate) fn run_from_shell(&self) -> anyhow::Result<()> {
        let mut args = vec!["build"];
        let cwd = self.root();
        let manifest = cwd.join("Cargo.toml");
        let target = self
            .target_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or(cwd.join("target"));
        if self.clean {
            std::fs::remove_dir_all(&target)
                .map_err(|e| anyhow!("{}: could not remove {:?}", e, target))?;
        }
        if let Some(s) = manifest.to_str() {
            args.push("--manifest-path");
            args.push(s);
        }
        if let Some(s) = &target.to_str() {
            args.push("--target-dir");
            args.push(s);
        }
        if self.quiet {
            args.push("--quiet");
        }
        if let Some(v) = self.verbosity {
            if v > 0 {
                args.push("--verbose");
            }
        }
        let features = self.features.as_ref().map(|v| v.join(","));
        if let Some(ref features) = features {
            args.push("--features");
            args.push(features);
        }
        if let Some(ref pkgid) = self.pkgid {
            args.push("--package");
            args.push(pkgid);
        }
        if let Some(ref profile) = self.profile {
            if profile != "release" {
                args.push("--profile");
                args.push(profile);
            } else {
                args.push("--release");
            }
        } else {
            args.push("--release");
        }
        if let Some(ref target) = self.target {
            args.push("--target");
            args.push(target);
        }
        if self.ignore_rust_version {
            args.push("--ignore-rust-version");
        }
        if self.keep_going {
            args.push("--keep-going");
        }
        if let Some(ref config) = self.config {
            for c in config {
                args.push("--config");
                args.push(&c);
            }
        }
        println!("cargo {}", args.join(" "));
        let out = std::process::Command::new("cargo")
            .args(&args)
            .current_dir(cwd)
            .stdout(std::io::stdout())
            .stderr(std::io::stderr())
            .output()?;
        if !out.status.success() {
            return Err(anyhow!("failed to build crate"));
        }
        Ok(())
    }

    pub(crate) fn from_path(p: &str) -> Self {
        Self {
            root: p.to_string(),
            pkgid: None,
            target_dir: None,
            profile: Some("release".to_string()),
            verbosity: Some(0),
            features: None,
            quiet: false,
            keep_going: false,
            ignore_rust_version: false,
            config: None,
            target: None,
            embeded_cargo: false,
            clean: false,
        }
    }

    pub(crate) fn from_path_and_table(p: &str, tbl: &mlua::Table<'_>) -> mlua::Result<Self> {
        Ok(Self {
            root: p.to_string(),
            pkgid: tbl.get("pkgid")?,
            target_dir: tbl.get("target_dir")?,
            profile: tbl.get("profile")?,
            verbosity: tbl.get("verbosity").ok(),
            features: tbl.get("features")?,
            quiet: tbl.get("quiet")?,
            keep_going: tbl.get("keep_going")?,
            ignore_rust_version: tbl.get("ignore_rust_version")?,
            config: tbl.get("config")?,
            target: tbl.get("target")?,
            embeded_cargo: tbl.get("embeded_cargo")?,
            clean: tbl.get("clean")?,
        })
    }

    pub(crate) fn bin(&self) -> PathBuf {
        let name = if let Some(ref pkgid) = self.pkgid {
            pkgid.clone()
        } else {
            PathBuf::from(&self.root)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        };
        if let Some(ref target) = self.target_dir {
            [target.clone(), self.profile_string(), name]
                .iter()
                .collect()
        } else {
            [
                self.root.clone(),
                "target".to_string(),
                self.profile_string(),
                name,
            ]
            .iter()
            .collect()
        }
    }

    fn root(&self) -> PathBuf {
        let mut p = PathBuf::from(&self.root);
        if p.is_relative() {
            p = std::env::current_dir().unwrap().join(p);
        }
        p
    }

    fn profile(&self) -> InternedString {
        self.profile_string().into()
    }

    fn profile_string(&self) -> String {
        if let Some(ref profile) = self.profile {
            profile.clone()
        } else {
            "release".to_string()
        }
    }

    #[allow(dead_code, unused_variables)]
    fn virtual_manifest(&self, config: &GlobalContext) -> VirtualManifest {
        let members = None;
        let default_members = None;
        let exclude = None;
        let inheritable = None;
        let custom_metadata = None;
        let ws_config = WorkspaceConfig::Root(WorkspaceRootConfig::new(
            &self.root(),
            &members,
            &default_members,
            &exclude,
            &inheritable,
            &custom_metadata,
        ));
        let features = Features::new(&[], config, &mut vec![], false).unwrap();
        // VirtualManifest::new(vec![], HashMap::new(), ws_config, None, features, None)
        unimplemented!()
    }
}

impl Default for Cargo {
    fn default() -> Self {
        Self::from_path(".")
    }
}
