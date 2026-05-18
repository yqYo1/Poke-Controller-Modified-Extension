//! Event handler trait and types

use crate::events::bus::Event;
use std::sync::Arc;

/// A handler function for events
pub type HandlerFn = Arc<dyn Fn(&mut Event) + Send + Sync>;

/// Trait for objects that can handle events
pub trait EventHandler: Send + Sync {
    /// Handle an event
    fn handle(&self, event: &mut Event);
}

impl<F> EventHandler for F
where
    F: Fn(&mut Event) + Send + Sync + 'static,
{
    fn handle(&self, event: &mut Event) {
        self(event)
    }
}
