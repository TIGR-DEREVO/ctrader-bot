// build.rs — запускается автоматически перед компиляцией проекта.
// Компилирует .proto файлы cTrader в Rust-код.
//
// .proto файлы нужно скачать отсюда:
// https://github.com/spotware/openapi-proto-messages

fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::Config::new()
        .compile_protos(
            &[
                "proto/OpenApiCommonModelMessages.proto",
                "proto/OpenApiCommonMessages.proto",
                "proto/OpenApiMessages.proto",
                "proto/OpenApiModelMessages.proto",
            ],
            &["proto/"],
        )?;

    // Говорим cargo: если .proto файлы изменились — перекомпилируй
    println!("cargo:rerun-if-changed=proto/");

    Ok(())
}
