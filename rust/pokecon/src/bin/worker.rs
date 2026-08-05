pub(crate) use pokecon::camera;
pub(crate) use pokecon::device;
pub(crate) use pokecon::worker;

#[path = "../worker_binary/mod.rs"]
mod worker_binary;

pub(crate) use worker_binary::dynamic;

fn main() -> Result<(), worker_binary::MainError> {
    worker_binary::main()
}
