variable "VERSION" {
    default = "0.0.1"
}

group "default" {
    targets = [
        "pax"
    ]
}

target "pax" {
    matrix = {
        item = [
            { RUST_VERSION = "1.84.0-bookworm", DEBIAN_VERSION = "bookworm" },
            { RUST_VERSION = "1.84.0-bullseye", DEBIAN_VERSION = "bullseye" },
        ]
    }
    name       = "pax_${replace(item.RUST_VERSION, ".", "-")}_${item.DEBIAN_VERSION}"
    context    = "."
    dockerfile = "Dockerfile"
    target     = "pax"
    tags = [
        "harrybrwn/pax:${VERSION}-${item.DEBIAN_VERSION}",
    ]
    args = {
        RUST_VERSION   = item.RUST_VERSION
        DEBIAN_VERSION = item.DEBIAN_VERSION
    }
}

target "pax-dist" {
    matrix = {
        item = [
            { RUST_VERSION = "1.84.0-bookworm", DEBIAN_VERSION = "bookworm" },
            { RUST_VERSION = "1.84.0-bullseye", DEBIAN_VERSION = "bullseye" },
        ]
    }
    output = ["type=local,dest=dist/${item.DEBIAN_VERSION}"]
    name       = "pax-dist_${replace(item.RUST_VERSION, ".", "-")}_${item.DEBIAN_VERSION}"
    context    = "."
    dockerfile = "Dockerfile"
    target     = "pax-dist"
    tags = [
        "harrybrwn/pax:${VERSION}-${item.DEBIAN_VERSION}",
    ]
    args = {
        RUST_VERSION   = item.RUST_VERSION
        DEBIAN_VERSION = item.DEBIAN_VERSION
    }
}
