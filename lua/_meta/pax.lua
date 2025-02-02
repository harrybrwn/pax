---@meta pax

error("Cannot require a meta file")

pax = {}

--- @class pax.BuildSpec
--- @field package string
--- @field version string
--- @field name? string
--- @field description? string
--- @field essential? boolean
--- @field author? string
--- @field email? string
--- @field maintainer? string
--- @field homepage? string
--- @field files pax.File[]
--- @field dependencies string[]
--- @field recommends? string[]
--- @field priority? string
--- @field arch string
--- @field urgency? string
--- @field apt_sources? any[]
--- @field scripts? pax.MaintainerScripts

--- @enum pax.Urgency
pax.Urgency = {
  Low = 0,
  Medium = 1,
  High = 2,
  Emergency = 3,
  Critical = 4,
}

--- @enum pax.Priority
pax.Priority = {
  Required = 0,
  Important = 1,
  Standard = 2,
  Optional = 3,
  Extra = 4,
  Invalid = 5,
}

--- @enum pax.Architecture
pax.Architecture = {
  All = 0,
  Any = 1,
  Source = 2,
  Invalid = 3,
}

--- @class pax.File
--- @field src string
--- @field dst string
--- @field mode number

--- @class pax.DownloadOpts
--- @field release? string
--- @field arch? string
--- @field out? string
--- @field compression? number 1 for gzip, 2 for xz

--- @class pax.Go
--- @field root      string  Root directory to run the go command in.
--- @field generate  boolean Run `go generate ./...` before building.
--- @field cmd?      string  Command directory to build. Default is '.'
--- @field out?      string
--- @field mode?     string
--- @field trimpath? boolean
--- @field ldflags?  string[] Pass additional flags to '-ldflags'
--- @field asmflags? string[] Pass additional flags to '-asmflags'
--- @field tags?     string[] Pass a list of tags to '-tags'
--- @field compiler? string

--- @class pax.Cargo
--- @field root string
--- @field pkgid? string
--- @field target_dir? string
--- @field profile? string
--- @field verbosity? number
--- @field features? string[]
--- @field quiet? boolean
--- @field keep_going? boolean
--- @field ignore_rust_version? boolean
--- @field config? string[]
--- @field target? string
--- @field embeded_cargo? boolean
--- @field clean? boolean

--- @class pax.MaintainerScripts
--- @field preinst? string
--- @field postinst? string
--- @field prerm? string
--- @field postrm? string

--- @class pax.SCDocOpts
--- @field input string
--- @field output string
--- @field compress? boolean

--- @class pax.Stat
--- @field size number
--- @field mode number
--- @field mtime number
--- @field atime number
--- @field ctime number
--- @field uid number
--- @field gid number
--- @field dev number
--- @field ino number
--- @field blocks number
--- @field blksize number
--- @field nlink number

--- @param spec table
--- @return pax.Project
function pax.project(spec) end

--- @return string
function pax.cwd() end

---@param ... any
function pax.print(...) end

--- @param message string
function pax.log(message) end

--- @param s string
--- @return number
function pax.octal(s) end

--- @param bin string
--- @param args? string[]
function pax.exec(bin, args) end

---@param script string
function pax.sh(script) end

-- Run the function inside of the specified directory.
--- @param dir string
--- @param fn function
function pax.in_dir(dir, fn) end

pax.git = {}

--- @class pax.GitCloneOpts
--- @field repo string
--- @field dest? string
--- @field branch? string
--- @field depth? number
--- @field force? boolean

--- @return string
function pax.git.email() end

--- @return string
function pax.git.username() end

--- @return string
function pax.git.version() end

--- @param repo string
--- @param opts? pax.GitCloneOpts
function pax.git.clone(repo, opts) end

pax.go = {}

--- @param go pax.Go
--- @return string[]
function pax.go.list(go) end

--- @param go pax.Go
function pax.go.build(go) end

--- @param go pax.Go
function pax.go.run(go) end

--- @param go pax.Go
function pax.go.generate(go) end

pax.cargo = {}

--- @param opts? pax.Cargo|string
function pax.cargo.build(opts) end

pax.dl = {}

--- @param url string
--- @param opts pax.DownloadOpts
function pax.dl.fetch(url, opts) end

--- @param opts pax.DownloadOpts
function pax.dl.kubectl(opts) end

--- @param opts pax.DownloadOpts
function pax.dl.jq(opts) end

--- @param opts pax.DownloadOpts
function pax.dl.youtube_dl(opts) end

--- @param opts pax.DownloadOpts
function pax.dl.yt_dlp(opts) end

--- @param opts pax.DownloadOpts
function pax.dl.mc(opts) end

--- @param opts pax.DownloadOpts
function pax.dl.tetris(opts) end

--- @param opts pax.DownloadOpts
function pax.dl.balena_etcher(opts) end

pax.fs = {}

--- @vararg string
function pax.fs.exists(...) end

--- @vararg string
function pax.fs.rm(...) end

--- @vararg string
function pax.fs.rmdir(...) end

--- @vararg string
function pax.fs.rmdir_all(...) end

--- @vararg string
function pax.fs.mkdir(...) end

--- @vararg string
function pax.fs.mkdir_all(...) end

--- @vararg string
function pax.fs.mkdir_force(...) end

--- @param dir string
--- @return pax.Stat
function pax.fs.stat(dir) end

pax.os = {}

--- @class pax.ExecOptions
--- @field dir?         string
--- @field stdout_file? string
--- @field stdin_file?  string

--- @param bin string
--- @param args? string[]
--- @param opts? pax.ExecOptions
--- @return number
function pax.os.exec(bin, args, opts) end

--- Locate a binary in the $PATH
--- @param name string
--- @return string
function pax.os.which(name) end

--- @class pax.LibcVersion
--- @field major number
--- @field minor number

--- Return the current version of libc.
--- @return pax.LibcVersion
function pax.os.libc_version() end

pax.path = {}

--- @vararg string
--- @return string
function pax.path.join(...) end

--- @param path string
--- @return string|nil
function pax.path.basename(path) end

--- @vararg any
--- @return boolean
function pax.path.is_absolute(...) end

--- @vararg any
--- @return boolean
function pax.path.is_relative() end

--- @vararg any
--- @return string
function pax.path.parent(...) end

---@class pax.Project
---@field base_dir     string
---@field man_dir      string
---@field version      string  Project's package version.
---@field package      string
---@field arch         string
---@field essential    boolean
---@field author?      string Package author.
---@field email?       string Package email.
---@field description? string
local Project = {}

function Project:build() end

function Project:finish() end

---@return string
function Project:dir() end

---@param path string
function Project:add_binary(path) end

--- @vararg pax.File
function Project:add_file(...) end

--- @param files pax.File[]
function Project:add_files(files) end

--- @param path string
function Project:merge_deb(path) end

---@param opts pax.Go
function Project:go_build(opts) end

--- @param opts pax.Cargo
function Project:cargo_build(opts) end

function Project:reset_build_number() end

function Project:enable_auto_build_numbers() end

--- @param url string
--- @param name? string
--- @param opts? pax.DownloadOpts
function Project:download_binary(url, name, opts) end

--- @param opts? pax.DownloadOpts
function Project:download_kubectl(opts) end

--- @param opts? pax.DownloadOpts
function Project:download_jq(opts) end

--- @param opts? pax.DownloadOpts
function Project:download_youtube_dl(opts) end

--- @param opts? pax.DownloadOpts
function Project:download_yt_dlp(opts) end

--- @param opts? pax.DownloadOpts
function Project:download_mc(opts) end

--- @param opts? pax.DownloadOpts
function Project:download_tetris(opts) end

--- @param opts? pax.DownloadOpts
function Project:download_balena_etcher(opts) end

--- @param opts pax.SCDocOpts
function Project:scdoc(opts) end

return pax
