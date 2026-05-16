//! Event handler trait and types

use crate::events::bus::Event;
use std::sync::Arc;

/// A handler function for events
pub type HandlerFn = Arc<dyn Fn(&Event) + Send + Sync>;

/// Trait for objects that can handle events
pub trait EventHandler: Send + Sync {
    /// Handle an event
    fn handle(&self, event: &Event);
}

impl<F> EventHandler for F
where
    F: Fn(&Event) + Send + Sync + 'static,
{
    fn handle(&self, event: &Event) {
        self(event)
    }
}
