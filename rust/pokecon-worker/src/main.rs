pub(crate) use pokecon_worker as worker;

#[path = "../../pokecon/src/worker_binary/mod.rs"]
mod worker_binary;

pub(crate) use worker_binary::dynamic;

fn main() -> Result<(), worker_binary::MainError> {
    worker_binary::main()
}
