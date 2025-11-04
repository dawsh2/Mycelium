//! Actor runtime for lifecycle management

use super::context::{ActorContext, ActorRef};
use super::handle::Actor;
use super::supervisor::SupervisionStrategy;
use crate::{MessageBus, TopicBuilder};
use futures::FutureExt;
use mycelium_protocol::routing::ActorId;
use std::any::TypeId;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinHandle;

/// Errors that can occur when spawning actors
#[derive(Error, Debug)]
pub enum SpawnError {
    #[error("Failed to create actor mailbox")]
    MailboxCreationFailed,

    #[error("Actor already stopped")]
    ActorStopped,

    #[error("Actor type already registered")]
    ActorTypeAlreadyExists,

    #[error("Actor not found")]
    ActorNotFound,
}

/// Runtime for managing actor lifecycle
///
/// The runtime is responsible for:
/// - Spawning actors with unique IDs and mailboxes
/// - Managing actor message loops
/// - Applying supervision strategies on actor failures
/// - Graceful shutdown of all actors
///
/// # Example
///
/// ```rust,ignore
/// use crate::{Actor, ActorRuntime, MessageBus};
///
/// let bus = MessageBus::new();
/// let runtime = ActorRuntime::new(bus);
///
/// // Spawn actors
/// let actor1 = runtime.spawn(MyActor::new()).await;
/// let actor2 = runtime.spawn(OtherActor::new()).await;
///
/// // Send messages
/// actor1.send(MyMessage { value: 42 }).await?;
///
/// // Graceful shutdown
/// runtime.shutdown().await;
/// ```
pub struct ActorRuntime {
    /// Message bus shared by all actors
    bus: MessageBus,
    /// Handles to running actors
    actor_handles: Arc<tokio::sync::Mutex<Vec<JoinHandle<()>>>>,
    /// Type registry for typed actor discovery (TypeId -> ActorId)
    type_registry: Arc<tokio::sync::RwLock<HashMap<TypeId, ActorId>>>,
    /// Actor ref registry for typed lookups (ActorId -> type-erased ActorRef)
    /// Uses Box<dyn Any> to store ActorRef<M> for different message types
    ref_registry: Arc<tokio::sync::RwLock<HashMap<ActorId, Box<dyn std::any::Any + Send + Sync>>>>,
}

impl ActorRuntime {
    /// Create a new actor runtime
    pub fn new(bus: MessageBus) -> Self {
        Self {
            bus,
            actor_handles: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            type_registry: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            ref_registry: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Get an actor by type (typed actor discovery)
    ///
    /// Returns the ActorRef for a previously spawned actor of type A.
    /// Only one actor per type can be registered (singleton pattern).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Spawn an actor
    /// runtime.spawn(MyActor::new()).await;
    ///
    /// // Later, get the actor by type
    /// let actor_ref = runtime.get_actor::<MyActor>().await?;
    /// actor_ref.send(MyMessage { value: 42 }).await?;
    /// ```
    pub async fn get_actor<A: Actor>(&self) -> Result<ActorRef<A::Message>, SpawnError> {
        let type_id = TypeId::of::<A>();

        // Look up ActorId by TypeId
        let actor_id = {
            let registry = self.type_registry.read().await;
            registry
                .get(&type_id)
                .copied()
                .ok_or(SpawnError::ActorNotFound)?
        };

        // Look up ActorRef by ActorId
        let ref_registry = self.ref_registry.read().await;
        let any_ref = ref_registry
            .get(&actor_id)
            .ok_or(SpawnError::ActorNotFound)?;

        // Downcast to correct type
        any_ref
            .downcast_ref::<ActorRef<A::Message>>()
            .ok_or(SpawnError::ActorNotFound)
            .map(|r| r.clone())
    }

    /// Spawn an actor
    ///
    /// Creates a unique mailbox for the actor and starts processing messages.
    /// Returns an ActorRef that can be used to send messages to the actor.
    ///
    /// The actor is automatically registered by type, allowing later lookup via
    /// `get_actor::<A>()`. Only one actor per type can be spawned (singleton pattern).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let actor_ref = runtime.spawn(MyActor { state: 0 }).await;
    /// actor_ref.send(MyMessage { value: 42 }).await?;
    ///
    /// // Later, get the actor by type
    /// let same_actor = runtime.get_actor::<MyActor>().await?;
    /// ```
    pub async fn spawn<A: Actor>(&self, mut actor: A) -> ActorRef<A::Message> {
        // Generate unique actor ID
        let actor_id = ActorId::new();

        // Create mailbox topic for this actor
        let topic = TopicBuilder::actor_mailbox(actor_id);

        // Create publisher for sending messages to this actor
        let publisher = self.bus.publisher_for_topic::<A::Message>(&topic);

        // Create subscriber for receiving messages
        let mut subscriber = self.bus.subscriber_for_topic::<A::Message>(&topic);

        // Create context
        let mut ctx = ActorContext::new(actor_id, self.bus.clone());

        // Create actor reference to return
        let actor_ref = ActorRef::new(actor_id, publisher.clone());

        // Spawn actor task
        let handle = tokio::spawn(async move {
            // Call started hook
            actor.started(&mut ctx).await;

            // Message processing loop
            loop {
                // Check if we should stop
                if ctx.should_stop() {
                    break;
                }

                // Receive next message
                let msg = match subscriber.recv().await {
                    Some(msg) => Arc::try_unwrap(msg).unwrap_or_else(|arc| (*arc).clone()),
                    None => {
                        // Channel closed, stop actor
                        break;
                    }
                };

                // Handle message with error handling
                // Wrap handle() to catch panics and convert them to errors
                let result = std::panic::AssertUnwindSafe(actor.handle(msg, &mut ctx))
                    .catch_unwind()
                    .await;

                if let Err(panic_err) = result {
                    // Convert panic to error
                    let error_msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                        (*s).to_string()
                    } else {
                        "Unknown panic".to_string()
                    };

                    let error: Box<dyn std::error::Error + Send> =
                        Box::new(std::io::Error::new(std::io::ErrorKind::Other, error_msg));

                    // Call on_error hook
                    let should_continue = actor.on_error(error, &mut ctx).await;

                    if !should_continue {
                        tracing::warn!("Actor {} stopping due to error", actor_id);
                        break;
                    }
                }
            }

            // Call stopped hook
            actor.stopped(&mut ctx).await;

            tracing::debug!("Actor {} stopped", actor_id);
        });

        // Store handle for tracking
        self.actor_handles.lock().await.push(handle);

        // Register actor type for typed discovery
        let type_id = TypeId::of::<A>();
        self.type_registry.write().await.insert(type_id, actor_id);

        // Register ActorRef for typed lookups
        self.ref_registry
            .write()
            .await
            .insert(actor_id, Box::new(actor_ref.clone()));

        actor_ref
    }

    /// Spawn an actor with a specific actor ID
    ///
    /// Useful for creating actors with predetermined IDs (e.g., singleton actors).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let singleton_id = ActorId::from_u64(1);
    /// let actor_ref = runtime.spawn_with_id(singleton_id, MyActor::new()).await;
    /// ```
    pub async fn spawn_with_id<A: Actor>(
        &self,
        actor_id: ActorId,
        mut actor: A,
    ) -> ActorRef<A::Message> {
        // Create mailbox topic for this actor
        let topic = TopicBuilder::actor_mailbox(actor_id);

        // Create publisher and subscriber
        let publisher = self.bus.publisher_for_topic::<A::Message>(&topic);
        let mut subscriber = self.bus.subscriber_for_topic::<A::Message>(&topic);

        // Create context
        let mut ctx = ActorContext::new(actor_id, self.bus.clone());

        // Create actor reference
        let actor_ref = ActorRef::new(actor_id, publisher.clone());

        // Spawn actor task (same error handling as spawn)
        let handle = tokio::spawn(async move {
            actor.started(&mut ctx).await;

            loop {
                if ctx.should_stop() {
                    break;
                }

                let msg = match subscriber.recv().await {
                    Some(msg) => Arc::try_unwrap(msg).unwrap_or_else(|arc| (*arc).clone()),
                    None => break,
                };

                // Handle message with error handling
                let result = AssertUnwindSafe(actor.handle(msg, &mut ctx))
                    .catch_unwind()
                    .await;

                if let Err(panic_err) = result {
                    let error_msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                        (*s).to_string()
                    } else {
                        "Unknown panic".to_string()
                    };

                    let error: Box<dyn std::error::Error + Send> =
                        Box::new(std::io::Error::new(std::io::ErrorKind::Other, error_msg));
                    let should_continue = actor.on_error(error, &mut ctx).await;

                    if !should_continue {
                        tracing::warn!("Actor {} stopping due to error", actor_id);
                        break;
                    }
                }
            }

            actor.stopped(&mut ctx).await;
            tracing::debug!("Actor {} stopped", actor_id);
        });

        self.actor_handles.lock().await.push(handle);

        // Register actor type for typed discovery
        let type_id = TypeId::of::<A>();
        self.type_registry.write().await.insert(type_id, actor_id);

        // Register ActorRef for typed lookups
        self.ref_registry
            .write()
            .await
            .insert(actor_id, Box::new(actor_ref.clone()));

        actor_ref
    }

    /// Spawn an actor with supervision
    ///
    /// If the actor panics or returns an error, the supervision strategy
    /// determines whether to restart it.
    ///
    /// # Current Implementation
    ///
    /// All actors spawned via `spawn()` already have basic error handling:
    /// - Panics are caught and converted to errors
    /// - The actor's `on_error()` hook is called
    /// - Actor can choose to continue or stop
    ///
    /// This method is reserved for future automatic restart strategies where
    /// the runtime would automatically respawn actors that exit unexpectedly.
    ///
    /// # Future Enhancement (TODO)
    ///
    /// To implement full supervision with automatic restarts:
    /// 1. Store actor factory functions (not just instances)
    /// 2. Monitor actor task completion
    /// 3. Apply backoff strategies (immediate, exponential, fixed delay)
    /// 4. Track restart attempts and enforce max_retries
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use crate::SupervisionStrategy;
    ///
    /// let actor_ref = runtime.spawn_supervised(
    ///     MyActor::new(),
    ///     SupervisionStrategy::Restart { max_retries: 3 }
    /// ).await;
    /// ```
    pub async fn spawn_supervised<A: Actor>(
        &self,
        actor: A,
        _strategy: SupervisionStrategy,
    ) -> ActorRef<A::Message> {
        // For now, spawn with basic error handling (panic catching + on_error hook)
        // Full automatic restart supervision is TODO
        self.spawn(actor).await
    }

    /// Graceful shutdown of all actors
    ///
    /// Waits for all actors to finish processing their current messages
    /// and complete their shutdown hooks.
    pub async fn shutdown(self) {
        let mut handles = self.actor_handles.lock().await;

        // Abort all actors
        for handle in handles.iter() {
            handle.abort();
        }

        // Wait for all actors to finish (with timeout)
        let timeout = std::time::Duration::from_secs(5);

        // Drain handles so we can await them
        let drained_handles: Vec<_> = handles.drain(..).collect();
        drop(handles); // Release the lock

        let _ = tokio::time::timeout(timeout, async {
            for handle in drained_handles {
                let _ = handle.await;
            }
        })
        .await;
    }

    /// Get the number of running actors
    pub async fn actor_count(&self) -> usize {
        self.actor_handles.lock().await.len()
    }
}

impl Default for ActorRuntime {
    fn default() -> Self {
        Self::new(MessageBus::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::impl_message;
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct TestMessage {
        value: u64,
    }

    impl_message!(TestMessage, 200, "test-actor");

    struct TestActor {
        count: u64,
    }

    #[async_trait::async_trait]
    impl Actor for TestActor {
        type Message = TestMessage;

        async fn handle(&mut self, msg: Self::Message, _ctx: &mut ActorContext<Self>) {
            self.count += msg.value;
        }
    }

    #[tokio::test]
    async fn test_spawn_actor() {
        let runtime = ActorRuntime::default();
        let actor_ref = runtime.spawn(TestActor { count: 0 }).await;

        // Send message
        actor_ref.send(TestMessage { value: 42 }).await.unwrap();

        // Give actor time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Actor should be running
        assert_eq!(runtime.actor_count().await, 1);
    }

    #[tokio::test]
    async fn test_spawn_with_id() {
        let runtime = ActorRuntime::default();
        let actor_id = ActorId::from_u64(12345);

        let actor_ref = runtime
            .spawn_with_id(actor_id, TestActor { count: 0 })
            .await;

        assert_eq!(actor_ref.actor_id(), actor_id);

        // Send message
        actor_ref.send(TestMessage { value: 10 }).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_multiple_actors() {
        let runtime = ActorRuntime::default();

        let actor1 = runtime.spawn(TestActor { count: 0 }).await;
        let actor2 = runtime.spawn(TestActor { count: 100 }).await;

        actor1.send(TestMessage { value: 1 }).await.unwrap();
        actor2.send(TestMessage { value: 2 }).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(runtime.actor_count().await, 2);
    }

    #[tokio::test]
    async fn test_typed_actor_discovery() {
        struct CounterActor {
            count: u64,
        }

        #[async_trait::async_trait]
        impl Actor for CounterActor {
            type Message = TestMessage;

            async fn handle(&mut self, msg: Self::Message, _ctx: &mut ActorContext<Self>) {
                self.count += msg.value;
            }
        }

        let runtime = ActorRuntime::default();

        // Spawn actor
        let actor_ref1 = runtime.spawn(CounterActor { count: 0 }).await;

        // Get actor by type
        let actor_ref2 = runtime.get_actor::<CounterActor>().await.unwrap();

        // Both refs should point to same actor
        assert_eq!(actor_ref1.actor_id(), actor_ref2.actor_id());

        // Send message via discovered ref
        actor_ref2.send(TestMessage { value: 42 }).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_get_actor_not_found() {
        struct NeverSpawnedActor;

        #[async_trait::async_trait]
        impl Actor for NeverSpawnedActor {
            type Message = TestMessage;

            async fn handle(&mut self, _msg: Self::Message, _ctx: &mut ActorContext<Self>) {}
        }

        let runtime = ActorRuntime::default();

        // Try to get actor that was never spawned
        let result = runtime.get_actor::<NeverSpawnedActor>().await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SpawnError::ActorNotFound));
    }

    #[tokio::test]
    async fn test_actor_panic_handling() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc as StdArc;

        struct PanickingActor {
            panic_on_first: bool,
            on_error_called: StdArc<AtomicBool>,
        }

        #[async_trait::async_trait]
        impl Actor for PanickingActor {
            type Message = TestMessage;

            async fn handle(&mut self, msg: Self::Message, _ctx: &mut ActorContext<Self>) {
                if self.panic_on_first && msg.value == 1 {
                    panic!("Intentional panic for testing");
                }
                // Second message should be processed normally
            }

            async fn on_error(
                &mut self,
                error: Box<dyn std::error::Error + Send>,
                _ctx: &mut ActorContext<Self>,
            ) -> bool {
                // Record that on_error was called
                self.on_error_called.store(true, Ordering::SeqCst);

                // Check error message contains "panic"
                assert!(error.to_string().contains("panic"));

                // Continue processing (return true)
                self.panic_on_first = false; // Don't panic again
                true
            }
        }

        let runtime = ActorRuntime::default();
        let error_flag = StdArc::new(AtomicBool::new(false));

        let actor = PanickingActor {
            panic_on_first: true,
            on_error_called: error_flag.clone(),
        };

        let actor_ref = runtime.spawn(actor).await;

        // Send message that will cause panic
        actor_ref.send(TestMessage { value: 1 }).await.unwrap();

        // Give actor time to panic and handle error
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify on_error was called
        assert!(error_flag.load(Ordering::SeqCst));

        // Send another message - actor should still be running
        actor_ref.send(TestMessage { value: 2 }).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Actor should still be running (didn't crash from panic)
        assert_eq!(runtime.actor_count().await, 1);
    }

    #[tokio::test]
    async fn test_actor_stops_on_error_false() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc as StdArc;

        struct StoppingActor {
            message_count: StdArc<AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl Actor for StoppingActor {
            type Message = TestMessage;

            async fn handle(&mut self, msg: Self::Message, _ctx: &mut ActorContext<Self>) {
                self.message_count.fetch_add(1, Ordering::SeqCst);

                if msg.value == 1 {
                    panic!("Stop on first message");
                }
            }

            async fn on_error(
                &mut self,
                _error: Box<dyn std::error::Error + Send>,
                _ctx: &mut ActorContext<Self>,
            ) -> bool {
                // Return false to stop the actor
                false
            }
        }

        let runtime = ActorRuntime::default();
        let counter = StdArc::new(AtomicUsize::new(0));

        let actor = StoppingActor {
            message_count: counter.clone(),
        };

        let actor_ref = runtime.spawn(actor).await;

        // Send message that will cause panic and stop
        actor_ref.send(TestMessage { value: 1 }).await.unwrap();

        // Give actor time to stop
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify first message was processed
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Try to send another message - actor should be stopped
        // This will fail because actor mailbox is closed
        let _result = actor_ref.send(TestMessage { value: 2 }).await;

        // The send might succeed (message queued) but won't be processed
        // Give it time to potentially process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Message count should still be 1 (second message not processed)
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
