//! Windows resources, and the build-time channel and version baked into the binary.
//!
//! `DESIGNCRAFT_BUILD_CHANNEL=nightly-v1` selects the Launcher's nightly-v1 flavor: it must come
//! with an exact `DESIGNCRAFT_VERSION` feed identity (`X.Y.Z-nightly.YYYYMMDD.<run>.<attempt>.g<sha12>`)
//! plus the matching `DESIGNCRAFT_BUILD_SHA` and `DESIGNCRAFT_BUILD_DATE`. Unset, empty or `stable`
//! is the stable build; anything else is a build error, so a runtime environment cannot promote it.
//!
//! The icon and VERSIONINFO go into `designcraft.exe` only. A missing resource compiler is a
//! warning, so a cross-compile from macOS or Linux still links, unless `DESIGNCRAFT_REQUIRE_WINRES=1`
//! (set by the release workflow) turns it into an error.

#[path = "src/build_channel.rs"]
mod build_channel;

fn die(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DESIGNCRAFT_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=DESIGNCRAFT_VERSION");
    println!("cargo:rerun-if-env-changed=DESIGNCRAFT_OVERRIDE_VERSION");
    println!("cargo:rerun-if-env-changed=DESIGNCRAFT_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=DESIGNCRAFT_BUILD_DATE");
    println!("cargo:rustc-check-cfg=cfg(designcraft_nightly_v1)");
    let nightly = match std::env::var("DESIGNCRAFT_BUILD_CHANNEL").as_deref() {
        Ok("nightly-v1") => {
            println!("cargo:rustc-cfg=designcraft_nightly_v1");
            true
        }
        // An empty value (an unset CI input) means the default stable build.
        Ok("stable" | "") | Err(std::env::VarError::NotPresent) => false,
        // A non-UTF-8 value is as unusable as an unknown one: refuse it instead of guessing.
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => die("unsupported DESIGNCRAFT_BUILD_CHANNEL (expected stable or nightly-v1)"),
    };
    let built_version = if nightly {
        // The shared workflow exports the prepared version as `DESIGNCRAFT_VERSION` and, for the
        // binary, an override of the same value; both must agree when both are set.
        let prepared = std::env::var("DESIGNCRAFT_VERSION").ok();
        let overridden = std::env::var("DESIGNCRAFT_OVERRIDE_VERSION").ok();
        if let (Some(prepared), Some(overridden)) = (&prepared, &overridden)
            && prepared != overridden
        {
            die("DESIGNCRAFT_VERSION and DESIGNCRAFT_OVERRIDE_VERSION disagree");
        }
        let Some(version) = overridden.or(prepared) else {
            die("nightly-v1 requires an explicit DESIGNCRAFT_VERSION feed identity");
        };
        if !build_channel::is_nightly_version(&version) {
            die("nightly-v1 requires DESIGNCRAFT_VERSION to be X.Y.Z-nightly.YYYYMMDD.<run>.<attempt>.g<sha12>");
        }
        let sha = std::env::var("DESIGNCRAFT_BUILD_SHA").unwrap_or_default();
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            die("nightly-v1 requires DESIGNCRAFT_BUILD_SHA to be the 40-hex source commit");
        }
        if build_channel::nightly_commit(&version) != Some(&sha[..12]) {
            die("DESIGNCRAFT_VERSION names a different commit than DESIGNCRAFT_BUILD_SHA");
        }
        let date = std::env::var("DESIGNCRAFT_BUILD_DATE").unwrap_or_default();
        let digits: Vec<char> = date.chars().filter(|c| c.is_ascii_digit()).collect();
        if date.len() != 10 || date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' || digits.iter().collect::<String>() != date.replace('-', "")
        {
            die("nightly-v1 requires DESIGNCRAFT_BUILD_DATE as YYYY-MM-DD");
        }
        if build_channel::nightly_date_digits(&version) != Some(&date.replace('-', "")) {
            die("DESIGNCRAFT_VERSION names a different date than DESIGNCRAFT_BUILD_DATE");
        }
        // A nightly-only name: `option_env!` reads the compiler's process environment, and stable
        // packaging always exports DESIGNCRAFT_BUILD_SHA, which must not turn a Stable build nightly.
        println!("cargo:rustc-env=DESIGNCRAFT_NIGHTLY_COMMIT={sha}");
        println!("cargo:rustc-env=DESIGNCRAFT_BUILD_DATE={date}");
        version
    } else {
        std::env::var("CARGO_PKG_VERSION").unwrap_or_default()
    };
    // One compile-time identity for `--version`, About and the session record.
    println!("cargo:rustc-env=DESIGNCRAFT_BUILT_VERSION={built_version}");

    println!("cargo:rerun-if-changed=../../assets/app-icon/designcraft.ico");
    println!("cargo:rerun-if-changed=../../assets/app-icon/designcraft-nightly.ico");
    println!("cargo:rerun-if-env-changed=DESIGNCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(build_channel::windows_icon_path(nightly))
        .set("ProductName", if nightly { "DesignCraft Nightly" } else { "DesignCraft" })
        .set("FileDescription", if nightly { "DesignCraft Nightly page layout" } else { "DesignCraft page layout" })
        .set("CompanyName", "Learning Machines LLC")
        .set("LegalCopyright", "Copyright (c) the DesignCraft authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "designcraft.exe")
        .set("InternalName", "designcraft");
    if nightly {
        // The Launcher checks the PE ProductVersion against the exact feed identity, not FileVersion.
        res.set("ProductVersion", &built_version);
    }
    if let Err(e) = res.compile() {
        if std::env::var_os("DESIGNCRAFT_REQUIRE_WINRES").is_some() {
            die(&format!("embedding Windows resources failed: {e}"));
        }
        println!("cargo:warning=designcraft.exe built without icon/version resources: {e}");
    }
}
