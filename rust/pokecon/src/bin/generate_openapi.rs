fn main() -> Result<(), Box<dyn std::error::Error>> {
    pokecon::binary_entrypoints::generate_openapi()
}
