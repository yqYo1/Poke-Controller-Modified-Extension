mod command;
mod events;
mod image_proc;
mod keys;
mod net;
mod notify;
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
    command::register(&cmd_module)?;
    m.add_submodule(&cmd_module)?;

    let sender_module = PyModule::new(m.py(), "sender")?;
    sender::register(&sender_module)?;
    m.add_submodule(&sender_module)?;

    let notify_module = PyModule::new(m.py(), "notify")?;
    notify::register(&notify_module)?;
    m.add_submodule(&notify_module)?;

    let net_module = PyModule::new(m.py(), "net")?;
    net::register(&net_module)?;
    m.add_submodule(&net_module)?;

    Ok(())
}
