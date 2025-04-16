fn main() {
    cxx_build::bridge("src/bindings/mod.rs")
        .file("src/bindings/nix.cpp")
        .flag("-std=c++2a")
        .flag("-O2")
        .flag("-include")
        .flag("nix/config.h")
        .flag("-I")
        .flag(concat!(env!("NIX_INCLUDE_PATH"), "/nix"))
        .compile("nixbinding");
    println!("cargo:rerun-if-changed=src/bindings");

    pkg_config::Config::new()
        .atleast_version("2.4")
        .probe("nix-store")
        .unwrap();
    pkg_config::Config::new()
        .atleast_version("2.4")
        .probe("nix-main")
        .unwrap();
}
