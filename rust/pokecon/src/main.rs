#[tokio::main]
async fn main() -> Result<(), pokecon::MainError> {
    pokecon::run_cli().await
}
