mod events;
mod image_proc;
mod keys;
mod python_cmd;
mod sender;

use pyo3::prelude::*;

#[pymodule]
fn pokecon(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let keys_module = PyModule::new(m.py(), "keys")?;
    keys::register(&keys_module)?;
    m.add_submodule(&keys_module)?;

    let events_module = PyModule::new(m.py(), "events")?;
    events::register(&events_module)?;
    m.add_submodule(&events_module)?;

    let image_module = PyModule::new(m.py(), "image_proc")?;
    image_proc::register(&image_module)?;
    m.add_submodule(&image_module)?;

    let cmd_module = PyModule::new(m.py(), "command")?;
    python_cmd::register(&cmd_module)?;
    m.add_submodule(&cmd_module)?;

    let sender_module = PyModule::new(m.py(), "sender")?;
    sender::register(&sender_module)?;
    m.add_submodule(&sender_module)?;

    Ok(())
}
