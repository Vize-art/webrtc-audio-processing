use anyhow::{Context, Result};
use std::{env, path::PathBuf, process::Command};

fn build_webrtc_for_android() -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let webrtc_source_dir = manifest_dir.join("webrtc-audio-processing");

    let android_ndk_home = env::var("ANDROID_NDK_HOME")
        .context("ANDROID_NDK_HOME must be set, either in your environment or a .env file.")?;

    let toolchains_path =
        PathBuf::from(&android_ndk_home).join("toolchains/llvm/prebuilt/linux-x86_64");
    let bin_path = toolchains_path.join("bin");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let api_level = "21";
    let target_triple = format!("{}-linux-android", target_arch);
    let full_target_triple = format!("{}{}", target_triple, api_level);

    let cc_path = bin_path.join(format!("{}-clang", full_target_triple));
    let cxx_path = bin_path.join(format!("{}-clang++", full_target_triple));
    let ar_path = bin_path.join("llvm-ar");

    let cross_file_path = out_dir.join("cross_file.txt");
    let cross_file_content = format!(
        r#"[binaries]
        c = '{}'
        cpp = '{}'
        ar = '{}'

        [host_machine]
        system = '{}'
        cpu_family = '{}'
        cpu = '{}'
        endian = 'little'"#,
        cc_path.to_str().unwrap(),
        cxx_path.to_str().unwrap(),
        ar_path.to_str().unwrap(),
        target_os,
        target_arch,
        target_arch,
    );
    std::fs::write(&cross_file_path, &cross_file_content)
        .context("Failed to write Meson cross file")?;

    let webrtc_build_dir = out_dir.join("webrtc-audio-processing");
    let meson_setup = Command::new("meson")
        .arg("setup")
        .arg("--prefix")
        .arg(&out_dir)
        .arg("-Ddefault_library=static")
        .arg("--cross-file")
        .arg(&cross_file_path)
        .arg(&webrtc_source_dir)
        .arg(&webrtc_build_dir)
        .arg("--wipe")
        .status()
        .context("Failed to execute meson. Do you have it installed?")?;
    assert!(meson_setup.success(), "Meson setup command failed.");

    let ninja_build = Command::new("ninja")
        .current_dir(&webrtc_build_dir)
        .status()
        .context("Failed to execute ninja. Do you have it installed?")?;
    assert!(ninja_build.success(), "Ninja build command failed.");

    let ninja_install = Command::new("ninja")
        .current_dir(&webrtc_build_dir)
        .arg("install")
        .status()
        .context("Failed to execute ninja install")?;
    assert!(ninja_install.success(), "Ninja install command failed.");

    Ok(())
}

fn main() -> Result<()> {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    if target_os == "android" {
        // dotenvy::dotenv().ok();
        build_webrtc_for_android()?;
    }

    let webrtc_source_root = manifest_dir.join("webrtc-audio-processing");
    let include_dirs = vec![
        out_dir.join("include"),
        webrtc_source_root.clone(),
        webrtc_source_root.join("webrtc"),
    ];
    let mut lib_dirs = vec![out_dir.join("lib")];

    // --- Start of fix ---
    // The Abseil libraries are built by Meson into a subdirectory. We need to find
    // that directory and add it to the linker search path.
    if target_os == "android" {
        let abseil_lib_dir =
            out_dir.join("webrtc-audio-processing/subprojects/abseil-cpp-20240722.0");
        lib_dirs.push(abseil_lib_dir);
    }
    // --- End of fix ---

    for dir in &lib_dirs {
        println!("cargo:rustc-link-search=native={}", dir.to_str().unwrap());
    }

    println!("cargo:rustc-link-lib=static=webrtc-audio-processing-2");
    // --- Start of fix ---
    // Link all the necessary Abseil libraries.
    println!("cargo:rustc-link-lib=static=absl_strings");
    println!("cargo:rustc-link-lib=static=absl_base");
    println!("cargo:rustc-link-lib=static=absl_flags");
    // --- End of fix ---

    if target_os == "android" {
        println!("cargo:rustc-link-lib=c++_shared");
    }

    cc::Build::new()
        .cpp(true)
        .file("src/wrapper.cpp")
        .includes(&include_dirs)
        .flag("-std=c++17")
        .flag("-Wno-unused-parameter")
        .flag("-Wno-deprecated-declarations")
        .out_dir(&out_dir)
        .compile("webrtc_audio_processing_wrapper");

    println!("cargo:rustc-link-lib=static=webrtc_audio_processing_wrapper");

    // --- Start of Corrected bindgen Code ---
    let mut builder = bindgen::Builder::default()
        .header("src/wrapper.hpp")
        .clang_args(&["-x", "c++", "-std=c++17"])
        .enable_cxx_namespaces()
        .allowlist_function("webrtc_audio_processing_wrapper::.*")
        .allowlist_type("webrtc::AudioProcessing.*") // Allowlist APM and its sub-structs
        .opaque_type("std::.*") // Treat std:: types as opaque
        .opaque_type("absl::.*") // Treat absl:: types as opaque. This is the fix.
        .derive_default(true)
        .derive_debug(true);
    // --- End of Corrected bindgen Code ---

    if target_os == "android" {
        let android_ndk_home = env::var("ANDROID_NDK_HOME").unwrap();
        let toolchains_path =
            PathBuf::from(&android_ndk_home).join("toolchains/llvm/prebuilt/linux-x86_64");
        let sysroot = toolchains_path.join("sysroot");

        let target = env::var("TARGET").unwrap();
        let api_level = "21";
        let full_target = format!("{}{}", target, api_level);

        builder = builder
            .clang_arg(format!("--sysroot={}", sysroot.to_str().unwrap()))
            .clang_arg(format!("--target={}", full_target));
    }

    for dir in &include_dirs {
        builder = builder.clang_arg(&format!("-I{}", dir.to_str().unwrap()));
    }

    builder
        .generate()
        .context("Unable to generate bindings")?
        .write_to_file(out_dir.join("bindings.rs"))
        .context("Couldn't write bindings!")?;

    Ok(())
}
