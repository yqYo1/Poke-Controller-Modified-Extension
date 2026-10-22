#[tokio::main]
async fn main() -> Result<(), pokecon::MainError> {
    Box::pin(pokecon::run_cli()).await
}
