use std::env;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    for f in ["pin_it.rc", "pin_it.manifest", "pin_it.ico"] {
        println!("cargo:rerun-if-changed=assets/{f}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    let ver = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut nums: Vec<String> = ver
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u16>().unwrap_or(0).to_string())
        .take(4)
        .collect();
    while nums.len() < 4 {
        nums.push("0".into());
    }
    let macros = [
        format!("VER_COMMA={}", nums.join(",")),
        format!("VER_STR=\"{ver}\""),
    ];
    if let Err(e) = embed_resource::compile("assets/pin_it.rc", macros).manifest_required() {
        panic!("embedding assets/pin_it.rc (icon, manifest, version info) failed: {e}");
    }
}
