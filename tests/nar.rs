use crate::common::HELLO_PATH;
use nix_compat::nixbase32;
use nixcp::make_nar::MakeNar;
use nixcp::path_info::PathInfo;
use sha2::Digest;
use tokio::io::AsyncReadExt;

mod common;

#[tokio::test]
async fn nar_size_and_hash() {
    let ctx = common::context();
    let path_info = PathInfo::from_path(HELLO_PATH, &ctx.store).await.unwrap();

    let mut nar = MakeNar::new(&path_info, ctx.store).unwrap();
    let mut reader = nar.compress_and_hash().unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).await.unwrap();
    drop(reader);

    assert_eq!(nar.nar_size, 234680);

    let nar_hash = nar.nar_hasher.finalize();
    let real_nar_hash = "08za7nnjda8kpdsd73v3mhykjvp0rsmskwsr37winhmzgm6iw79w";
    assert_eq!(nixbase32::encode(nar_hash.as_slice()), real_nar_hash);
}
