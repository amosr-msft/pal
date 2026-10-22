use std::process::Command;
use zngur::Zngur;

fn llvm_config_flags(args: &[&str]) -> Vec<String> {
    (Command::new("llvm-config-20").args(args.iter()).output())
        .or(Command::new("llvm-config").args(args.iter()).output())
        .expect("Cannot find llvm-config")
        .stdout
        .split(|c| c.is_ascii_whitespace())
        .filter(|flag| flag.len() > 0)
        .map(|flag| std::str::from_utf8(flag).unwrap().into())
        .collect()
}

fn split_off<'a>(prefix: &str, s: &'a str) -> Option<&'a str> {
    if s.starts_with(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn take_until(s: &str, c: char) -> &str {
    s.find(c).map(|i| &s[..i]).unwrap_or(s)
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(pal_nightly_let_chains)");
    let rustc = std::env::var_os("RUSTC").expect("Cargo must set RUSTC");
    let version = Command::new(rustc)
        .arg("--version")
        .output()
        .expect("Cannot query Rust compiler version");
    assert!(
        version.status.success(),
        "Cannot query Rust compiler version"
    );
    let version = std::str::from_utf8(&version.stdout).expect("Invalid Rust compiler version");
    let minor = version
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.split('.').nth(1))
        .and_then(|v| v.parse::<u32>().ok())
        .expect("Cannot parse Rust compiler version");
    if minor < 88 {
        println!("cargo::rustc-cfg=pal_nightly_let_chains");
    }

    build_rs::output::rerun_if_changed("cpp/iface.zng");
    build_rs::output::rerun_if_changed("cpp/impl.cpp");

    let crate_dir = build_rs::input::cargo_manifest_dir();
    let out_dir = build_rs::input::out_dir();

    Zngur::from_zng_file(crate_dir.join("cpp/iface.zng"))
        .with_cpp_file(out_dir.join("generated.cpp"))
        .with_h_file(crate_dir.join("cpp/generated.h"))
        .with_rs_file(out_dir.join("generated.rs"))
        .generate();

    // Zngur 0.5 emits edition 2021 FFI syntax.
    let generated_rs = out_dir.join("generated.rs");
    let generated = std::fs::read_to_string(&generated_rs)
        .expect("Cannot read generated Rust FFI")
        .replace("#[no_mangle]", "#[unsafe(no_mangle)]")
        .replace("extern \"C\" {", "unsafe extern \"C\" {");
    std::fs::write(generated_rs, generated).expect("Cannot update generated Rust FFI");

    let mut build = cc::Build::new();

    let llvm_version = llvm_config_flags(&["--version"])
        .first()
        .and_then(|v| str::parse::<u32>(take_until(v, '.')).ok())
        .expect("Cannot parse `llvm-config --version` output");
    if llvm_version < 20 {
        panic!(
            "\nPAL requires clang >= 20, but found version {:}\n",
            llvm_version
        )
    }

    // zngur warnings...
    build
        .flag("-Wno-undefined-inline")
        .flag("-Wno-uninitialized-const-reference")
        .flag("-Wno-unused-parameter");

    build
        .cpp(true)
        .compiler("clang++")
        .include(crate_dir.join("cpp"));

    build
        .clone()
        .file(out_dir.join("generated.cpp"))
        .flag("-Wno-unused-function")
        .compile("zngur_generated");

    for flag in llvm_config_flags(&["--cxxflags"]) {
        build.flag(flag);
    }
    build.file("cpp/impl.cpp").compile("impls");

    for flag in llvm_config_flags(&["--ldflags", "--libs"]) {
        if let Some(lib) = split_off("-l", &flag) {
            build_rs::output::rustc_link_lib(lib);
        } else if let Some(lib_path) = split_off("-L", &flag) {
            build_rs::output::rustc_link_search(lib_path);
        }
    }
    build_rs::output::rustc_link_lib("clang-cpp");
}
