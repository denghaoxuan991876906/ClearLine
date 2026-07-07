use std::env;
use std::fs;
use std::path::{Path, PathBuf};

struct PayloadSpec {
    relative_path: String,
    source_path: PathBuf,
}

fn main() {
    println!("cargo:rerun-if-changed=ClearLineSetup.exe.manifest");
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest_file("ClearLineSetup.exe.manifest")
            .expect("embed ClearLineSetup.exe.manifest");
    }

    println!("cargo:rerun-if-env-changed=CLEARLINE_SETUP_STRICT_PAYLOAD");
    println!("cargo:rerun-if-env-changed=CLEARLINE_INSTALLER_HELPER_EXE");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .expect("clearline-setup must live in the repo root")
        .to_path_buf();
    let strict = env::var("CLEARLINE_SETUP_STRICT_PAYLOAD").ok().as_deref() == Some("1");
    let helper_exe = env::var_os("CLEARLINE_INSTALLER_HELPER_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo_root
                .join("target")
                .join("release")
                .join("clearline-installer-helper.exe")
        });

    let mut payloads = vec![
        PayloadSpec {
            relative_path: "ClearLine.exe".to_owned(),
            source_path: repo_root.join("dist").join("ClearLine.exe"),
        },
        PayloadSpec {
            relative_path: "models/deepfilternet/enc.onnx".to_owned(),
            source_path: repo_root
                .join("dist")
                .join("models")
                .join("deepfilternet")
                .join("enc.onnx"),
        },
        PayloadSpec {
            relative_path: "models/deepfilternet/erb_dec.onnx".to_owned(),
            source_path: repo_root
                .join("dist")
                .join("models")
                .join("deepfilternet")
                .join("erb_dec.onnx"),
        },
        PayloadSpec {
            relative_path: "models/deepfilternet/df_dec.onnx".to_owned(),
            source_path: repo_root
                .join("dist")
                .join("models")
                .join("deepfilternet")
                .join("df_dec.onnx"),
        },
        PayloadSpec {
            relative_path: "models/deepfilternet/config.ini".to_owned(),
            source_path: repo_root
                .join("dist")
                .join("models")
                .join("deepfilternet")
                .join("config.ini"),
        },
        PayloadSpec {
            relative_path: "models/deepfilternet/source.json".to_owned(),
            source_path: repo_root
                .join("dist")
                .join("models")
                .join("deepfilternet")
                .join("source.json"),
        },
        PayloadSpec {
            relative_path: "installer/clearline-installer-helper.exe".to_owned(),
            source_path: helper_exe,
        },
    ];

    let vb_cable_zip = repo_root
        .join("third_party")
        .join("vb-cable")
        .join("VBCABLE_Driver_Pack45.zip");
    payloads.push(PayloadSpec {
        relative_path: "virtual-audio/vb-cable/VBCABLE_Driver_Pack45.zip".to_owned(),
        source_path: vb_cable_zip,
    });

    let mut generated = String::from(
        "pub struct PayloadFile { pub relative_path: &'static str, pub bytes: &'static [u8] }\n\npub static PAYLOAD_FILES: &[PayloadFile] = &[\n",
    );
    let mut total_bytes = 0u64;
    let mut count = 0usize;
    let mut missing = Vec::new();

    for payload in payloads {
        println!("cargo:rerun-if-changed={}", payload.source_path.display());
        if !payload.source_path.is_file() {
            missing.push(format!(
                "{} <- {}",
                payload.relative_path.as_str(),
                payload.source_path.display()
            ));
            continue;
        }
        let source_path = payload.source_path.clone();
        let size = fs::metadata(&source_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        total_bytes += size;
        count += 1;
        generated.push_str(&format!(
            "    PayloadFile {{ relative_path: {:?}, bytes: include_bytes!(r#\"{}\"#) }},\n",
            payload.relative_path,
            path_for_include_bytes(&source_path)
        ));
    }

    if strict && !missing.is_empty() {
        panic!(
            "ClearLine setup payload is incomplete:\n{}",
            missing.join("\n")
        );
    }

    for missing_entry in &missing {
        println!("cargo:warning=Skipping missing ClearLine setup payload: {missing_entry}");
    }

    generated.push_str("] ;\n");
    generated.push_str(&format!("pub const PAYLOAD_FILE_COUNT: usize = {count};\n"));
    generated.push_str(&format!(
        "pub const PAYLOAD_TOTAL_BYTES: u64 = {total_bytes};\n"
    ));

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("payload_manifest.rs"), generated).expect("write payload manifest");
}

fn path_for_include_bytes(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
}
