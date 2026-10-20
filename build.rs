// Builds MacViewer's G.722 decoder (WebRTC upstream plus the NAudio output-wrap shim).
fn main() {
    println!("cargo:rerun-if-changed=third_party/g722");
    cc::Build::new()
        .include("third_party/g722/include")
        .file("third_party/g722/g722_decode.c")
        .file("third_party/g722/MacViewerG722.c")
        .compile("g722");
}
