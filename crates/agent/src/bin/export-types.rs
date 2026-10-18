fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/src/api/generated");
    agent::export_types(&directory)
}
