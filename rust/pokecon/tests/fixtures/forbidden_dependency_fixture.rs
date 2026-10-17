// AR-11-36 intentional reverse-flow fixture
// File-as-text positive control for `forbidden_dependency_edges_are_rejected`.
// It is NOT a Cargo target and is never compiled: the test reads it with
// `repository_text` and runs the shared `use`-line extractor over its lines.
// The single import below models a worker module reaching up into the server
// layer, which the forbidden manifest rejects.
use crate::server::state::StateHub;
