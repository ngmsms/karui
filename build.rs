fn main() {
    // exe とウィンドウのアイコンを埋め込む
    embed_resource::compile("assets/karui.rc", embed_resource::NONE).manifest_optional().unwrap();
}
