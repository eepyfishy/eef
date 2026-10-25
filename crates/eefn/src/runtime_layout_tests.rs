use super::*;

struct Fixture {
    _dir: tempfile::TempDir,
    install: PathBuf,
    bundle: PathBuf,
    layout: RuntimeLayout,
}
fn file(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"fixture, never executable").unwrap();
}
fn fixture(versioned: bool, marked: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    let bundle = if versioned {
        install.join("versions/0.4.0-test")
    } else {
        install.clone()
    };
    let executable = bundle.join("eefn.exe");
    file(&executable);
    if marked {
        file(&bundle.join("bundle.json"));
    }
    let layout = RuntimeLayout::from_executable(&executable).unwrap();
    // Resolver uses canonical directories (including Windows extended prefixes).
    Fixture {
        _dir: dir,
        install: install.canonicalize().unwrap(),
        bundle: bundle.canonicalize().unwrap(),
        layout,
    }
}
fn python(bundle: &Path) -> PathBuf {
    bundle.join(if cfg!(windows) {
        "python/python.exe"
    } else {
        "python/bin/python3"
    })
}
fn binary() -> &'static Path {
    Path::new("tools/llama-server.exe")
}

#[test]
fn managed_default_configuration_stays_in_installation_before_first_creation() {
    for (versioned, marked) in [(false, true), (true, false), (true, true)] {
        let f = fixture(versioned, marked);
        for default in [
            Path::new("config.json"),
            Path::new("config/default_identity.yaml"),
        ] {
            assert_eq!(
                f.layout.configuration(default, default),
                f.install.join(default)
            );
            assert!(!f.install.join(default).exists());
        }
        let custom = Path::new("owner/custom.yaml");
        assert_eq!(
            f.layout.configuration(custom, Path::new("config.json")),
            custom
        );
    }
}

#[test]
fn unmanaged_configuration_keeps_development_lookup_and_explicit_paths() {
    let f = fixture(false, false);
    let default = Path::new("config.json");
    assert_eq!(f.layout.configuration(default, default), default);
    file(&f.install.join(default));
    assert_eq!(
        f.layout.configuration(default, default),
        f.install.join(default)
    );
    let explicit = f._dir.path().join("owner.json");
    assert_eq!(f.layout.configuration(&explicit, default), explicit);
}

#[test]
fn implicit_managed_data_default_follows_installation_without_migrating_external_configs() {
    let default = Path::new("config/default_identity.yaml");
    let relative = Path::new("data/firmware");
    for (versioned, marked) in [(false, true), (true, false), (false, false)] {
        let f = fixture(versioned, marked);
        let config = f.install.join(default);
        file(&config);
        let config = config.canonicalize().unwrap();
        assert_eq!(
            f.layout.default_data_directory(&config, default, relative),
            if versioned || marked {
                f.install.join(relative)
            } else {
                relative.to_path_buf()
            }
        );
        let external = f._dir.path().join("owner/custom.yaml");
        assert_eq!(
            f.layout
                .default_data_directory(&external, default, relative),
            relative
        );
        assert_eq!(
            f.layout
                .default_data_directory(Path::new(""), default, relative),
            relative
        );
    }
}

#[test]
fn bundled_components_flat_package_keeps_its_own_files() {
    let f = fixture(false, true);
    let py = python(&f.bundle);
    file(&py);
    let model = f.bundle.join(binary());
    file(&model);
    let plugin = f.bundle.join("python/plugins/audio_output.py");
    file(&plugin);
    assert_eq!(f.layout.bundled_python(), Some(py));
    assert_eq!(
        f.layout
            .model_binary(&f.install.join("config.json"), binary(), binary()),
        model
    );
    assert_eq!(
        f.layout.builtin_plugin("audio_output.py", None).unwrap(),
        plugin
    );
}

#[test]
fn bundled_components_selected_version_does_not_prefer_old_installation_files() {
    let f = fixture(true, true);
    file(&python(&f.install));
    file(&python(&f.bundle));
    file(&f.install.join(binary()));
    file(&f.bundle.join(binary()));
    file(&f.install.join("python/plugins/audio_output.py"));
    file(&f.bundle.join("python/plugins/audio_output.py"));
    assert_eq!(f.layout.bundled_python(), Some(python(&f.bundle)));
    assert_eq!(
        f.layout
            .model_binary(&f.install.join("config.json"), binary(), binary()),
        f.bundle.join(binary())
    );
    assert_eq!(
        f.layout
            .builtin_plugin("audio_output.py", Some(&f.install))
            .unwrap(),
        f.bundle.join("python/plugins/audio_output.py")
    );
}

#[test]
fn bundled_components_pending_application_selection_cannot_select_components_ahead_of_handoff() {
    let f = fixture(true, true);
    let pending = f.install.join("versions/0.4.99");
    file(&python(&f.bundle));
    file(&f.bundle.join(binary()));
    file(&python(&pending));
    file(&pending.join(binary()));
    std::fs::write(f.install.join("current.txt"), "0.4.99\n").unwrap();
    let layout = RuntimeLayout::from_executable(f.bundle.join("eefn.exe")).unwrap();
    assert_eq!(layout.bundled_python(), Some(python(&f.bundle)));
    assert_eq!(
        layout.model_binary(&f.install.join("config.json"), binary(), binary()),
        f.bundle.join(binary())
    );
}

#[test]
fn bundled_components_missing_selected_files_do_not_borrow_old_root_or_source_tree() {
    let f = fixture(true, false);
    file(&python(&f.install));
    file(&f.install.join(binary()));
    file(&f.install.join("python/plugins/audio_output.py"));
    assert_eq!(f.layout.bundled_python(), Some(python(&f.bundle)));
    assert_eq!(
        f.layout
            .model_binary(&f.install.join("config.json"), binary(), binary()),
        f.bundle.join(binary())
    );
    assert!(
        f.layout
            .builtin_plugin("audio_output.py", Some(&f.install))
            .is_err()
    );
}

#[test]
fn bundled_components_owner_binary_overrides_and_external_config_paths_are_preserved() {
    let f = fixture(true, true);
    let custom = f.install.join("custom/owner-runtime.exe");
    file(&custom);
    assert_eq!(
        f.layout
            .model_binary(&f.install.join("config.json"), &custom, binary()),
        custom
    );
    let owner_relative = Path::new("custom/owner-runtime.exe");
    assert_eq!(
        f.layout
            .model_binary(&f.install.join("config.json"), owner_relative, binary()),
        f.install.join(owner_relative)
    );
    let external = f._dir.path().join("external");
    std::fs::create_dir_all(&external).unwrap();
    file(&external.join(binary()));
    assert_eq!(
        f.layout
            .model_binary(&external.join("config.json"), binary(), binary()),
        external.join(binary())
    );
    let other = Path::new("other-runtime.exe");
    assert_eq!(
        f.layout
            .model_binary(&external.join("config.json"), other, binary()),
        f.install.join(other)
    );
}

#[test]
fn bundled_components_unmarked_development_retains_explicit_source_fallback() {
    let f = fixture(false, false);
    let source = f._dir.path().join("source");
    let plugin = source.join("python/plugins/audio_output.py");
    file(&plugin);
    assert!(!f.layout.managed);
    assert!(f.layout.bundled_python().is_none());
    assert_eq!(
        f.layout
            .builtin_plugin("audio_output.py", Some(&source))
            .unwrap(),
        plugin
    );
}

#[test]
fn bundled_components_malformed_marker_does_not_enable_implicit_fallback() {
    let f = fixture(false, false);
    std::fs::create_dir(f.bundle.join("bundle.json")).unwrap();
    let layout = RuntimeLayout::from_executable(f.bundle.join("eefn.exe")).unwrap();
    let source = f._dir.path().join("source");
    file(&source.join("python/plugins/audio_output.py"));
    assert!(layout.managed);
    assert_eq!(layout.bundled_python(), Some(python(&f.bundle)));
    assert!(
        layout
            .builtin_plugin("audio_output.py", Some(&source))
            .is_err()
    );
}

#[test]
fn bundled_components_configuration_paths_are_not_implicit_working_directory_paths() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("node/config.json");
    assert_eq!(
        config_path(&config, Path::new("python/custom.exe")),
        dir.path().join("node/python/custom.exe")
    );
    let absolute = dir.path().join("explicit-python.exe");
    assert_eq!(config_path(&config, &absolute), absolute);
    assert_eq!(
        config_path(Path::new("config.json"), Path::new("custom.exe")),
        PathBuf::from("./custom.exe")
    );
}

#[test]
fn bundled_components_builtin_names_cannot_escape_bundle_directory() {
    let f = fixture(true, true);
    for invalid in [
        "",
        ".",
        "..",
        "../audio_output.py",
        "python/audio_output.py",
    ] {
        assert!(
            f.layout.builtin_plugin(invalid, Some(&f.install)).is_err(),
            "{invalid}"
        );
    }
    assert!(
        f.layout
            .builtin_plugin(f.install.to_str().unwrap(), None)
            .is_err()
    );
}
