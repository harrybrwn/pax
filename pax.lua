local pax = require("pax")

local function file(s, d, mode)
	return {
		src = s,
		dst = d,
		mode = pax.octal(mode),
	}
end

-- pax:add({
--   package = "pax",
--   version = "1.0.0",
--   arch = "amd64",
--   description = "test build object",
--   author = pax.git.username(),
--   email = pax.git.email(),
--   files = {
--     file("./target/release/pax", "/usr/bin/pax", "0775"),
--     file("README.md", "/usr/share/pax/README.md", "0644"),
--     "pax.lua:/usr/share/pax/example.lua",
--   },
--   dependencies = {},
--   urgency = pax.Urgency.Critical,
-- })

pax.cargo.build({
	".",
	pkgid = "pax",
	verbosity = 0,
	profile = "release",
	config = {
		"profile.release.strip='symbols'",
	},
})

pax:package_crate("./pax", {
	arch = "amd64",
	files = {
		file("README.md", "/usr/share/pax/README.md", "0644"),
		"pax.lua:/usr/share/pax/example.lua",
	},
})
