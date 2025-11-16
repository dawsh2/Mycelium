use mycelium_protocol::{codec::HEADER_SIZE, SCHEMA_DIGEST};
use mycelium_transport::{config::Topology, MessageBus};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::slice;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Opaque runtime handle shared with foreign callers.
#[repr(C)]
pub struct MyceliumRuntime {
    bus: MessageBus,
    runtime: Runtime,
    tx: mpsc::UnboundedSender<RuntimeCommand>,
    subscriptions: Mutex<HashMap<u64, SubscriptionHandle>>,
    next_subscription_id: AtomicU64,
    publish_worker: Option<JoinHandle<()>>,
}

pub const SUCCESS: i32 = 0;
pub const ERR_NULL: i32 = -1;
pub const ERR_DIGEST: i32 = -2;
pub const ERR_RUNTIME: i32 = -3;
pub const ERR_SUBSCRIPTION: i32 = -4;
pub const ERR_TOPOLOGY: i32 = -5;
pub const ERR_SERVICE_NOT_FOUND: i32 = -6;

type MessageCallback = unsafe extern "C" fn(u16, *const u8, usize, *mut c_void);

struct SubscriptionHandle {
    join: JoinHandle<()>,
}

impl SubscriptionHandle {
    fn stop(self) {
        self.join.abort();
    }
}

enum RuntimeCommand {
    Publish { type_id: u16, payload: Vec<u8> },
}

/// Create a fresh runtime with its own MessageBus and Tokio executor.
#[no_mangle]
pub extern "C" fn mycelium_runtime_create() -> *mut MyceliumRuntime {
    create_runtime_internal(MessageBus::new())
}

/// Create a runtime from a topology file.
///
/// # Arguments
///
/// * `topology_path` - Path to topology.toml file (null-terminated C string)
/// * `service_name` - Name of this service in the topology (null-terminated C string)
///
/// # Returns
///
/// Pointer to MyceliumRuntime, or null if creation failed
#[no_mangle]
pub unsafe extern "C" fn mycelium_runtime_create_from_topology(
    topology_path: *const c_char,
    service_name: *const c_char,
) -> *mut MyceliumRuntime {
    if topology_path.is_null() || service_name.is_null() {
        return std::ptr::null_mut();
    }

    let Ok(path_str) = CStr::from_ptr(topology_path).to_str() else {
        return std::ptr::null_mut();
    };

    let Ok(service_str) = CStr::from_ptr(service_name).to_str() else {
        return std::ptr::null_mut();
    };

    // Load topology
    let topology = match Topology::load(path_str) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to load topology: {}", e);
            return std::ptr::null_mut();
        }
    };

    // Validate service exists
    if topology.find_node(service_str).is_none() {
        tracing::error!("Service '{}' not found in topology", service_str);
        return std::ptr::null_mut();
    }

    // Create MessageBus from topology
    let bus = match MessageBus::from_topology(topology, service_str) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to create MessageBus from topology: {}", e);
            return std::ptr::null_mut();
        }
    };

    create_runtime_internal(bus)
}

fn create_runtime_internal(bus: MessageBus) -> *mut MyceliumRuntime {
    match Runtime::new() {
        Ok(runtime) => {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let bus_clone = bus.clone();
            let worker = runtime.spawn(async move {
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        RuntimeCommand::Publish { type_id, payload } => {
                            if let Err(err) = bus_clone.publish_raw(type_id, &payload) {
                                tracing::warn!("native publish failed: {err}");
                            }
                        }
                    }
                }
            });

            let runtime_handle = MyceliumRuntime {
                bus,
                runtime,
                tx,
                subscriptions: Mutex::new(HashMap::new()),
                next_subscription_id: AtomicU64::new(1),
                publish_worker: Some(worker),
            };

            Box::into_raw(Box::new(runtime_handle))
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy a runtime created via `mycelium_runtime_create`.
#[no_mangle]
pub unsafe extern "C" fn mycelium_runtime_destroy(handle: *mut MyceliumRuntime) {
    if handle.is_null() {
        return;
    }
    let mut boxed = Box::from_raw(handle);
    if let Ok(mut subs) = boxed.subscriptions.lock() {
        for (_, handle) in subs.drain() {
            handle.stop();
        }
    }
    if let Some(worker) = boxed.publish_worker.take() {
        worker.abort();
    }
    drop(boxed);
}

/// Verify that a runtime matches the compiled schema digest.
#[no_mangle]
pub unsafe extern "C" fn mycelium_verify_schema_digest(
    handle: *mut MyceliumRuntime,
    digest_ptr: *const u8,
    digest_len: usize,
) -> i32 {
    if handle.is_null() || digest_ptr.is_null() {
        return ERR_NULL;
    }

    if digest_len != SCHEMA_DIGEST.len() {
        return ERR_DIGEST;
    }

    let provided = std::slice::from_raw_parts(digest_ptr, digest_len);
    if provided == SCHEMA_DIGEST {
        SUCCESS
    } else {
        ERR_DIGEST
    }
}

/// Publish a TLV payload into the local MessageBus.
#[no_mangle]
pub unsafe extern "C" fn mycelium_publish(
    handle: *mut MyceliumRuntime,
    type_id: u16,
    payload_ptr: *const u8,
    payload_len: usize,
) -> i32 {
    if handle.is_null() || payload_ptr.is_null() {
        return ERR_NULL;
    }

    let payload = slice::from_raw_parts(payload_ptr, payload_len).to_vec();
    let runtime = &*handle;
    match runtime
        .tx
        .send(RuntimeCommand::Publish { type_id, payload })
    {
        Ok(_) => SUCCESS,
        Err(_) => ERR_RUNTIME,
    }
}

/// Register a callback for raw TLV payloads of a given type.
#[no_mangle]
pub unsafe extern "C" fn mycelium_subscribe(
    handle: *mut MyceliumRuntime,
    type_id: u16,
    callback: Option<MessageCallback>,
    user_data: *mut c_void,
    out_subscription_id: *mut u64,
) -> i32 {
    if handle.is_null() || out_subscription_id.is_null() {
        return ERR_NULL;
    }
    let Some(callback) = callback else {
        return ERR_NULL;
    };

    let runtime = &*handle;
    let mut stream = runtime.bus.subscribe_raw();
    let subscription_id = runtime.next_subscription_id.fetch_add(1, Ordering::Relaxed);
    let cb = callback;
    let user_data_addr = user_data as usize;
    let join = runtime.runtime.spawn(async move {
        while let Some(raw) = stream.recv().await {
            if raw.type_id() != type_id {
                continue;
            }
            let storage = raw.into_tlv();
            if storage.len() <= HEADER_SIZE {
                continue;
            }
            let payload_ptr = unsafe { storage.as_ptr().add(HEADER_SIZE) };
            let payload_len = storage.len() - HEADER_SIZE;
            unsafe {
                cb(
                    type_id,
                    payload_ptr,
                    payload_len,
                    user_data_addr as *mut c_void,
                );
            }
            drop(storage);
        }
    });

    match runtime.subscriptions.lock() {
        Ok(mut subs) => {
            subs.insert(subscription_id, SubscriptionHandle { join });
            *out_subscription_id = subscription_id;
            SUCCESS
        }
        Err(_) => {
            join.abort();
            ERR_RUNTIME
        }
    }
}

/// Remove a subscription previously returned by [`mycelium_subscribe`].
#[no_mangle]
pub unsafe extern "C" fn mycelium_unsubscribe(
    handle: *mut MyceliumRuntime,
    subscription_id: u64,
) -> i32 {
    if handle.is_null() {
        return ERR_NULL;
    }
    let runtime = &*handle;
    match runtime.subscriptions.lock() {
        Ok(mut subs) => match subs.remove(&subscription_id) {
            Some(handle) => {
                handle.stop();
                SUCCESS
            }
            None => ERR_SUBSCRIPTION,
        },
        Err(_) => ERR_RUNTIME,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mycelium_protocol::{impl_message, Message};
    use std::{ffi::c_void, sync::mpsc, time::Duration};
    use zerocopy::{AsBytes, FromBytes, FromZeroes};

    #[derive(Debug, Clone, Copy, PartialEq, AsBytes, FromBytes, FromZeroes)]
    #[repr(C)]
    struct FfiTestMsg {
        value: u64,
    }

    impl_message!(FfiTestMsg, 9001, "ffi-test");

    #[test]
    fn digest_check_passes() {
        let handle = mycelium_runtime_create();
        assert!(!handle.is_null());
        let code = unsafe {
            mycelium_verify_schema_digest(handle, SCHEMA_DIGEST.as_ptr(), SCHEMA_DIGEST.len())
        };
        assert_eq!(code, SUCCESS);
        unsafe { mycelium_runtime_destroy(handle) };
    }

    #[test]
    fn digest_check_fails() {
        let handle = mycelium_runtime_create();
        assert!(!handle.is_null());
        let bad = [0xAAu8; 32];
        let code = unsafe { mycelium_verify_schema_digest(handle, bad.as_ptr(), bad.len()) };
        assert_eq!(code, ERR_DIGEST);
        unsafe { mycelium_runtime_destroy(handle) };
    }

    #[test]
    fn publish_round_trip() {
        let handle = mycelium_runtime_create();
        assert!(!handle.is_null());
        let runtime = unsafe { &*handle };
        // Ensure type registered by creating publisher/subscriber once
        let _ = runtime.bus.publisher::<FfiTestMsg>();
        let mut sub = runtime.bus.subscriber::<FfiTestMsg>();

        let msg = FfiTestMsg { value: 42 };
        let payload = msg.as_bytes();
        let code = unsafe {
            mycelium_publish(handle, FfiTestMsg::TYPE_ID, payload.as_ptr(), payload.len())
        };
        assert_eq!(code, SUCCESS);

        let received = runtime
            .runtime
            .block_on(async { sub.recv().await })
            .expect("recv");
        assert_eq!(received.value, 42);

        unsafe { mycelium_runtime_destroy(handle) };
    }

    extern "C" fn record_value_callback(
        _type_id: u16,
        payload_ptr: *const u8,
        payload_len: usize,
        user_data: *mut c_void,
    ) {
        assert!(!payload_ptr.is_null());
        assert!(payload_len >= std::mem::size_of::<u64>());
        let sender = unsafe { &*(user_data as *const mpsc::Sender<u64>) };
        let bytes = unsafe { std::slice::from_raw_parts(payload_ptr, payload_len) };
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&bytes[..8]);
        let value = u64::from_le_bytes(buf);
        sender.send(value).expect("send callback value");
    }

    #[test]
    fn subscribe_receives_rust_publish() {
        let handle = mycelium_runtime_create();
        assert!(!handle.is_null());
        let runtime = unsafe { &*handle };

        let (tx, rx) = mpsc::channel::<u64>();
        let tx_ptr = Box::into_raw(Box::new(tx));

        let mut sub_id = 0u64;
        let code = unsafe {
            mycelium_subscribe(
                handle,
                FfiTestMsg::TYPE_ID,
                Some(record_value_callback),
                tx_ptr.cast::<c_void>(),
                &mut sub_id,
            )
        };
        assert_eq!(code, SUCCESS);
        assert_ne!(sub_id, 0);

        let publisher = runtime.bus.publisher::<FfiTestMsg>();
        runtime
            .runtime
            .block_on(async { publisher.publish(FfiTestMsg { value: 7 }).await })
            .expect("publish succeeds");

        let received = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("callback value");
        assert_eq!(received, 7);

        let unsub = unsafe { mycelium_unsubscribe(handle, sub_id) };
        assert_eq!(unsub, SUCCESS);

        unsafe {
            drop(Box::from_raw(tx_ptr));
            mycelium_runtime_destroy(handle);
        }
    }
}
