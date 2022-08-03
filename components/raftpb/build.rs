use std::io::Result;

fn main() -> Result<()> {
    let mut build = prost_build::Config::new();
    build.out_dir("src");
    build.compile_protos(&["proto/raftpb.proto"], &["proto"])?;
    Ok(())
}