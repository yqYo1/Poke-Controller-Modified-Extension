use std::path::PathBuf;

const TEST_WORKER_BINARY_ENV: &str = "POKECON_TEST_WORKER_BINARY";

pub fn worker_binary() -> PathBuf {
    std::env::var_os(TEST_WORKER_BINARY_ENV).map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_pokecon-worker")),
        |binary| {
            let binary = PathBuf::from(binary);
            assert!(
                binary.is_absolute(),
                "{TEST_WORKER_BINARY_ENV} must name an absolute worker binary"
            );
            binary
        },
    )
}
