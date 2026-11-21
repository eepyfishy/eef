use std::{env, io, path::PathBuf, process::Command};

pub fn embed(product: &str, description: &str) -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../shared/windows_version.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    let mut resource = winresource::WindowsResource::new();
    resource
        .set("ProductName", product)
        .set("FileDescription", description)
        .set("OriginalFilename", &format!("{product}.exe"))
        .set("CompanyName", "eef contributors")
        .set("LegalCopyright", "eef contributors. MIT licensed.");
    match env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => resource.compile(),
        Ok("gnu") => {
            let directory = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
            let script = directory.join("version.rc");
            let object = directory.join("version.o");
            resource.write_resource_file(&script)?;

            println!("cargo:rerun-if-env-changed=WINDRES");
            let compiler = env::var_os("WINDRES").unwrap_or_else(|| "windres".into());
            let mut command = Command::new(compiler);
            let target = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
                Ok("x86_64") => Some("pe-x86-64"),
                Ok("x86") => Some("pe-i386"),
                _ => None,
            };
            if let Some(target) = target {
                command.args(["--target", target]);
            }
            let status = command
                .args(["-O", "coff"])
                .arg(script)
                .arg(&object)
                .status()?;
            if !status.success() {
                return Err(io::Error::other(
                    "Could not compile Windows version metadata",
                ));
            }
            // Linking a resource library would also put the node's metadata in
            // the coordinator, which uses the node crate as a library.
            println!("cargo:rustc-link-arg-bin={product}={}", object.display());
            Ok(())
        }
        _ => Err(io::Error::other(
            "Unsupported Windows resource compiler target",
        )),
    }
}
