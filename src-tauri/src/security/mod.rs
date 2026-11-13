//! Security configuration. License and Keychain handling arrive in Phase 7.
//!
//! The tests here guard the least-privilege configuration (NFR-011, NFR-012,
//! SAFETY_MODEL S1/S3/S11) so it cannot be loosened by accident.

#[cfg(test)]
mod config_tests {
    const CAPABILITIES: &str = include_str!("../../capabilities/default.json");
    const TAURI_CONF: &str = include_str!("../../tauri.conf.json");
    const CLIPPY_TOML: &str = include_str!("../../clippy.toml");
    const CARGO_TOML: &str = include_str!("../../Cargo.toml");

    #[test]
    fn r_sec_01_webview_has_no_fs_shell_http_or_opener_permissions() {
        let caps: serde_json::Value = serde_json::from_str(CAPABILITIES).expect("valid json");
        let perms = caps["permissions"].as_array().expect("permissions array");
        for p in perms {
            let id = p
                .as_str()
                .or_else(|| p["identifier"].as_str())
                .unwrap_or_default();
            for banned in ["fs:", "shell:", "http:", "opener:", "process:"] {
                assert!(
                    !id.starts_with(banned),
                    "capability grants banned permission {id}"
                );
            }
        }
        for banned in ["tauri-plugin-fs", "tauri-plugin-shell", "tauri-plugin-http"] {
            assert!(
                !CARGO_TOML.contains(banned),
                "{banned} must not be a dependency"
            );
        }
    }

    #[test]
    fn r_sec_01b_csp_is_strict() {
        let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).expect("valid json");
        let csp = conf["app"]["security"]["csp"]
            .as_str()
            .expect("csp must be set");
        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("script-src 'self'"));
        assert!(
            !csp.contains("script-src 'self' 'unsafe"),
            "no unsafe script sources in production"
        );
        assert!(
            !csp.contains("http://*") && !csp.contains("https:"),
            "no remote origins"
        );
        assert_eq!(conf["app"]["security"]["freezePrototype"], true);
    }

    #[test]
    fn r_sec_02_clippy_bans_dangerous_fs_and_process_calls() {
        for path in [
            "std::fs::rename",
            "std::fs::copy",
            "std::fs::write",
            "std::fs::remove_file",
            "std::fs::remove_dir_all",
            "std::fs::File::create",
            "tokio::fs::rename",
            "tokio::fs::remove_file",
            "std::process::Command::new",
        ] {
            assert!(
                CLIPPY_TOML.contains(&format!("path = \"{path}\"")),
                "clippy.toml must ban {path}"
            );
        }
        assert!(CARGO_TOML.contains("unsafe_code = \"deny\""));
    }
}
