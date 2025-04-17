use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path(out_dir.join("advanced_server.bin"))
        .compile_protos(
            &["proto/advanced_server/v1.proto"],
            &["proto/advanced_server"],
        )?;
    Ok(())
}
