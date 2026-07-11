use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use bevy_ecs::{message::Message, system::Command, world::World};
use gpui::{App, Global};

trait DeferredBevyMutation: Send {
    fn apply(self: Box<Self>, world: &mut World);
}

struct QueuedCommand<C>(C);

impl<C> DeferredBevyMutation for QueuedCommand<C>
where
    C: Command<Out = ()>,
{
    fn apply(self: Box<Self>, world: &mut World) {
        self.0.apply(world);
    }
}

struct QueuedMessage<M>(M);

impl<M: Message> DeferredBevyMutation for QueuedMessage<M> {
    fn apply(self: Box<Self>, world: &mut World) {
        world.write_message(self.0);
    }
}

#[derive(Clone, Default)]
pub(crate) struct BevyMutationQueue(Arc<Mutex<VecDeque<Box<dyn DeferredBevyMutation>>>>);

impl BevyMutationQueue {
    fn push(&self, mutation: impl DeferredBevyMutation + 'static) {
        self.0.lock().unwrap().push_back(Box::new(mutation));
    }

    pub(crate) fn drain_into(&self, world: &mut World) {
        loop {
            let mutation = self.0.lock().unwrap().pop_front();
            let Some(mutation) = mutation else {
                break;
            };
            mutation.apply(world);
        }
    }
}

pub(crate) struct BevyBridgeGlobal(pub(crate) BevyMutationQueue);

impl Global for BevyBridgeGlobal {}

/// Deferred, alias-safe writes from retained GPUI callbacks into Bevy.
pub trait BevyAppContextExt {
    /// Publishes a Bevy message at the next safe main-world boundary.
    fn send_bevy_message<M: Message>(&mut self, message: M);

    /// Applies a Bevy command at the next safe main-world boundary.
    fn queue_bevy_command<C: Command<Out = ()>>(&mut self, command: C);
}

impl BevyAppContextExt for App {
    fn send_bevy_message<M: Message>(&mut self, message: M) {
        self.global::<BevyBridgeGlobal>()
            .0
            .push(QueuedMessage(message));
    }

    fn queue_bevy_command<C: Command<Out = ()>>(&mut self, command: C) {
        self.global::<BevyBridgeGlobal>()
            .0
            .push(QueuedCommand(command));
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::Resource;

    use super::*;

    #[derive(Resource, Default)]
    struct Counter(u32);

    #[test]
    fn deferred_commands_apply_only_when_the_bridge_is_drained() {
        let queue = BevyMutationQueue::default();
        let mut world = World::new();
        world.init_resource::<Counter>();
        queue.push(QueuedCommand(|world: &mut World| {
            world.resource_mut::<Counter>().0 += 1;
        }));

        assert_eq!(world.resource::<Counter>().0, 0);
        queue.drain_into(&mut world);
        assert_eq!(world.resource::<Counter>().0, 1);
    }
}
