fn main() {
    let cfg = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", cfg).unwrap();
    // Windows exe ikonu (MSVC'de rc.exe, MinGW'de windres kullanılır)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("app.rc", embed_resource::NONE);
    }
}
