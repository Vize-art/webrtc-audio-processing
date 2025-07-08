use anyhow::{Context, Result};
use std::{env, path::PathBuf, process::Command};

fn build_webrtc_for_ios(target_triple: &str, target_arch: &str) -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let webrtc_source_dir = manifest_dir.join("webrtc-audio-processing");

    let is_simulator = target_arch == "x86_64" || target_triple.contains("-sim");

    let (sdk_name, min_version_flag) = if is_simulator {
        ("iphonesimulator", "-miphonesimulator-version-min=12.0")
    } else {
        ("iphoneos", "-miphoneos-version-min=12.0")
    };

    let clang_arch = if target_arch == "aarch64" { "arm64" } else { target_arch };

    let sdk_path_output = Command::new("xcrun")
        .args(["--sdk", sdk_name, "--show-sdk-path"])
        .output()
        .context(format!("Failed to find iOS SDK path for '{}'. Is Xcode installed?", sdk_name))?;
    if !sdk_path_output.status.success() {
        anyhow::bail!("xcrun command failed: {}", String::from_utf8_lossy(&sdk_path_output.stderr));
    }
    let sdk_path = String::from_utf8(sdk_path_output.stdout)?.trim().to_string();

    let target_triple_clang = match target_triple {
        "aarch64-apple-ios" => "aarch64-apple-ios",
        "x86_64-apple-ios" | "x86_64-apple-ios-sim" => "x86_64-apple-ios-simulator",
        "aarch64-apple-ios-sim" => "aarch64-apple-ios-simulator",
        _ => anyhow::bail!("Unsupported iOS target for WebRTC build: {}", target_triple),
    };

    let cross_file_path = out_dir.join(format!("cross_file_{}.txt", target_triple));
    let cross_file_content = format!(
        r#"[binaries]
        c = ['clang', '--target={target_clang}', '-arch', '{clang_arch}', '--sysroot={sdk_path}', '{min_version_flag}']
        cpp = ['clang++', '--target={target_clang}', '-arch', '{clang_arch}', '--sysroot={sdk_path}', '{min_version_flag}']
        ar = 'ar'
        strip = 'strip'
        c_build = 'clang'
        cpp_build = 'clang++'

        [host_machine]
        system = 'ios'
        cpu_family = '{target_arch}'
        cpu = '{target_arch}'
        endian = 'little'

        [properties]
        sys_root = '{sdk_path}'
        "#,
        target_clang = target_triple_clang,
        clang_arch = clang_arch,
        sdk_path = &sdk_path,
        min_version_flag = min_version_flag,
        target_arch = target_arch,
    );
    std::fs::write(&cross_file_path, cross_file_content)
        .context("Failed to write Meson cross file for iOS")?;

    let webrtc_build_dir = out_dir.join(format!("webrtc-build-{}", target_triple));
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
        .context("Failed to execute meson for iOS. Is it installed?")?;
    assert!(meson_setup.success(), "Meson setup command for iOS failed.");

    let ninja_build = Command::new("ninja")
        .current_dir(&webrtc_build_dir)
        .status()
        .context("Failed to execute ninja for iOS. Is it installed?")?;
    assert!(ninja_build.success(), "Ninja build command for iOS failed.");

    let ninja_install = Command::new("ninja")
        .current_dir(&webrtc_build_dir)
        .arg("install")
        .status()
        .context("Failed to execute ninja install for iOS")?;
    assert!(ninja_install.success(), "Ninja install command for iOS failed.");

    Ok(())
}

fn build_webrtc_for_android() -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let webrtc_source_dir = manifest_dir.join("webrtc-audio-processing");

    let android_ndk_home = env::var("ANDROID_NDK_HOME")
        .context("ANDROID_NDK_HOME must be set, either in your environment or a .env file.")?;

    let toolchains_path =
        PathBuf::from(android_ndk_home).join("toolchains/llvm/prebuilt/linux-x86_64");
    let bin_path = toolchains_path.join("bin");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let api_level = "21";
    let target_triple = format!("{}-linux-android", target_arch);
    let full_target_triple = format!("{}{}", target_triple, api_level);

    let cc_path = bin_path.join(format!("{}-clang", full_target_triple));
    let cxx_path = bin_path.join(format!("{}-clang++", full_target_triple));
    let ar_path = bin_path.join("llvm-ar");

    let cross_file_path = out_dir.join("cross_file_android.txt");
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
        .context("Failed to write Meson cross file for Android")?;

    let webrtc_build_dir = out_dir.join("webrtc-build-android");
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
        .context("Failed to execute meson for Android. Do you have it installed?")?;
    assert!(meson_setup.success(), "Meson setup command for Android failed.");

    let ninja_build = Command::new("ninja")
        .current_dir(&webrtc_build_dir)
        .status()
        .context("Failed to execute ninja for Android. Is it installed?")?;
    assert!(ninja_build.success(), "Ninja build command for Android failed.");

    let ninja_install = Command::new("ninja")
        .current_dir(&webrtc_build_dir)
        .arg("install")
        .status()
        .context("Failed to execute ninja install for Android")?;
    assert!(ninja_install.success(), "Ninja install command for Android failed.");

    Ok(())
}

fn main() -> Result<()> {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let target_triple = env::var("TARGET").unwrap();

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    if target_os == "android" {
        build_webrtc_for_android()?;
    } else if target_os == "ios" {
        build_webrtc_for_ios(&target_triple, &target_arch)?;
    }

    let webrtc_source_root = manifest_dir.join("webrtc-audio-processing");
    let include_dirs = vec![
        out_dir.join("include"),
        webrtc_source_root.clone(),
        webrtc_source_root.join("webrtc"),
    ];

    let lib_dir = out_dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.to_str().unwrap());

    println!("cargo:rustc-link-lib=static=webrtc-audio-processing-2");

    if target_os == "android" {
        println!("cargo:rustc-link-lib=c++_shared");
    } else if target_os == "ios" {
        println!("cargo:rustc-link-lib=c++");
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

    let mut builder = bindgen::Builder::default()
        .header("src/wrapper.hpp")
        .clang_args(&["-x", "c++", "-std=c++17"])
        .enable_cxx_namespaces()
        .allowlist_function("webrtc_audio_processing_wrapper::.*")
        .allowlist_type("webrtc::AudioProcessing.*")
        .opaque_type("std::.*")
        .opaque_type("absl::.*")
        .derive_default(true)
        .derive_debug(true);

    if target_os == "android" {
        let android_ndk_home = env::var("ANDROID_NDK_HOME").unwrap();
        let toolchains_path =
            PathBuf::from(android_ndk_home).join("toolchains/llvm/prebuilt/linux-x86_64");
        let sysroot = toolchains_path.join("sysroot");

        let api_level = "21";
        let full_target = format!("{}{}", target_triple, api_level);

        builder = builder
            .clang_arg(format!("--sysroot={}", sysroot.to_str().unwrap()))
            .clang_arg(format!("--target={}", full_target));
    } else if target_os == "ios" {
        let is_simulator = target_arch == "x86_64" || target_triple.contains("-sim");
        let sdk_name = if is_simulator { "iphonesimulator" } else { "iphoneos" };
        let sdk_path_output =
            Command::new("xcrun").args(["--sdk", sdk_name, "--show-sdk-path"]).output()?;
        let sdk_path = String::from_utf8(sdk_path_output.stdout)?.trim().to_string();

        let target_clang = match target_triple.as_str() {
            "aarch64-apple-ios" => "aarch64-apple-ios",
            "x86_64-apple-ios" | "x86_64-apple-ios-sim" => "x86_64-apple-ios-simulator",
            "aarch64-apple-ios-sim" => "aarch64-apple-ios-simulator",
            _ => anyhow::bail!("Unsupported iOS target for bindgen: {}", target_triple),
        };

        builder = builder
            .clang_arg(format!("--sysroot={}", sdk_path))
            .clang_arg(format!("--target={}", target_clang));
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
