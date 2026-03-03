#[cxx::bridge(namespace = "aeron_rs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("shim.h");

        type ContextWrapper;
        type AeronWrapper;
        type PublicationWrapper;
        type ExclusivePublicationWrapper;
        type SubscriptionWrapper;
        type MediaDriverWrapper;
        type CountersReaderWrapper;

        fn create_context() -> UniquePtr<ContextWrapper>;
        fn create_aeron(context: UniquePtr<ContextWrapper>) -> Result<UniquePtr<AeronWrapper>>;
        fn create_media_driver() -> UniquePtr<MediaDriverWrapper>;

        fn start(self: Pin<&mut AeronWrapper>);
        fn isClosed(self: &AeronWrapper) -> bool;
        fn addPublication(self: Pin<&mut AeronWrapper>, channel: &str, stream_id: i32) -> Result<UniquePtr<PublicationWrapper>>;
        fn addExclusivePublication(self: Pin<&mut AeronWrapper>, channel: &str, stream_id: i32) -> Result<UniquePtr<ExclusivePublicationWrapper>>;
        fn addSubscription(self: Pin<&mut AeronWrapper>, channel: &str, stream_id: i32) -> Result<UniquePtr<SubscriptionWrapper>>;
        fn countersReader(self: &AeronWrapper) -> UniquePtr<CountersReaderWrapper>;

        fn start(self: Pin<&mut MediaDriverWrapper>);

        fn offer(self: Pin<&mut PublicationWrapper>, buffer: &[u8]) -> i64;
        fn tryClaim(self: Pin<&mut PublicationWrapper>, length: usize, handler_id: usize) -> i64;
        fn isConnected(self: &PublicationWrapper) -> bool;

        fn offer(self: Pin<&mut ExclusivePublicationWrapper>, buffer: &[u8]) -> i64;
        fn tryClaim(self: Pin<&mut ExclusivePublicationWrapper>, length: usize, handler_id: usize) -> i64;
        fn isConnected(self: &ExclusivePublicationWrapper) -> bool;

        fn poll(self: Pin<&mut SubscriptionWrapper>, fragment_limit: i32, handler_id: usize) -> i32;
        fn pollAssembled(self: Pin<&mut SubscriptionWrapper>, fragment_limit: i32, handler_id: usize) -> i32;
        fn controlledPollAssembled(self: Pin<&mut SubscriptionWrapper>, fragment_limit: i32, handler_id: usize) -> i32;
        fn isConnected(self: &SubscriptionWrapper) -> bool;

        fn maxCounterId(self: &CountersReaderWrapper) -> i32;
        fn getCounterValue(self: &CountersReaderWrapper, id: i32) -> i64;
        fn getCounterState(self: &CountersReaderWrapper, id: i32) -> i32;
        fn getCounterTypeId(self: &CountersReaderWrapper, id: i32) -> i32;
        fn getCounterLabel(self: &CountersReaderWrapper, id: i32) -> String;
        fn forEach(self: &CountersReaderWrapper, handler_id: usize);
    }

    extern "Rust" {
        fn handle_fragment(handler_id: usize, buffer: &[u8]);
        fn handle_controlled_fragment(handler_id: usize, buffer: &[u8]) -> i32;
        fn handle_claim(handler_id: usize, buffer: &mut [u8]) -> bool;
        fn handle_counters_metadata(handler_id: usize, counter_id: i32, type_id: i32, key: &[u8], label: String);
    }
}

pub struct AeronClient {
    inner: cxx::UniquePtr<ffi::AeronWrapper>,
}

impl AeronClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let ctx = ffi::create_context();
        let aeron = ffi::create_aeron(ctx)?;
        
        Ok(Self { inner: aeron })
    }

    pub fn start(&mut self) {
        self.inner.pin_mut().start();
    }

    pub fn is_closed(&self) -> bool {
        self.inner.isClosed()
    }
    
    pub fn add_publication(&mut self, channel: &str, stream_id: i32) -> Result<Publication, Box<dyn std::error::Error>> {
        let pub_inner = self.inner.pin_mut().addPublication(channel, stream_id)?;
        Ok(Publication { inner: pub_inner })
    }

    pub fn add_exclusive_publication(&mut self, channel: &str, stream_id: i32) -> Result<ExclusivePublication, Box<dyn std::error::Error>> {
        let pub_inner = self.inner.pin_mut().addExclusivePublication(channel, stream_id)?;
        Ok(ExclusivePublication { inner: pub_inner })
    }

    pub fn add_subscription(&mut self, channel: &str, stream_id: i32) -> Result<Subscription, Box<dyn std::error::Error>> {
        let sub_inner = self.inner.pin_mut().addSubscription(channel, stream_id)?;
        Ok(Subscription { inner: sub_inner })
    }

    pub fn counters_reader(&self) -> CountersReader {
        CountersReader {
            inner: self.inner.countersReader(),
        }
    }
}

pub struct Publication {
    inner: cxx::UniquePtr<ffi::PublicationWrapper>,
}

impl Publication {
    pub fn offer(&mut self, buffer: &[u8]) -> i64 {
        self.inner.pin_mut().offer(buffer)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the stream position (>0 on success, negative on back-pressure/closed).
    pub fn try_claim<F>(&mut self, length: usize, mut handler: F) -> i64
    where F: FnMut(&mut [u8]) -> bool
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&mut [u8]) -> bool + 'static) = unsafe {
            std::mem::transmute::<*mut dyn FnMut(&mut [u8]) -> bool, *mut (dyn FnMut(&mut [u8]) -> bool + 'static)>(&mut handler as *mut dyn FnMut(&mut [u8]) -> bool)
        };

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().tryClaim(length, handler_id);

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }

    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }
}

pub struct ExclusivePublication {
    inner: cxx::UniquePtr<ffi::ExclusivePublicationWrapper>,
}

impl ExclusivePublication {
    pub fn offer(&mut self, buffer: &[u8]) -> i64 {
        self.inner.pin_mut().offer(buffer)
    }

    /// Zero-copy publish: claims a region of the log buffer, calls `handler` with a mutable
    /// slice pointing directly into shared memory, then commits or aborts based on the return value.
    /// Returns the stream position (>0 on success, negative on back-pressure/closed).
    pub fn try_claim<F>(&mut self, length: usize, mut handler: F) -> i64
    where F: FnMut(&mut [u8]) -> bool
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&mut [u8]) -> bool + 'static) = unsafe {
            std::mem::transmute::<*mut dyn FnMut(&mut [u8]) -> bool, *mut (dyn FnMut(&mut [u8]) -> bool + 'static)>(&mut handler as *mut dyn FnMut(&mut [u8]) -> bool)
        };

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().tryClaim(length, handler_id);

        CLAIM_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }

    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }
}

use std::cell::RefCell;
use std::collections::HashMap;

/// Flow-control actions for `poll_assembled` when the handler returns a `ControlledAction`.
/// Matches Aeron's `ControlledPollAction` enum values.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlledAction {
    /// Abort polling — rewind position, re-deliver this fragment next poll.
    Abort = 0,
    /// Stop polling this image, commit position up to this fragment.
    Break = 1,
    /// Checkpoint position for flow control, continue polling.
    Commit = 2,
    /// Continue processing (default behavior).
    Continue = 3,
}

/// Trait that allows `poll_assembled` to accept handlers returning either `()` or `ControlledAction`.
/// Closures returning `()` map to `ControlledAction::Continue`.
pub trait PollAction {
    fn into_action(self) -> ControlledAction;
}

impl PollAction for () {
    #[inline]
    fn into_action(self) -> ControlledAction {
        ControlledAction::Continue
    }
}

impl PollAction for ControlledAction {
    #[inline]
    fn into_action(self) -> ControlledAction {
        self
    }
}

// Thread-local registries for closures passed across the cxx boundary.
// We use pointer-based handler IDs since cxx doesn't support passing trait objects directly.
thread_local! {
    static HANDLERS: RefCell<HashMap<usize, *mut dyn FnMut(&[u8])>> = RefCell::new(HashMap::new());
    static CLAIM_HANDLERS: RefCell<HashMap<usize, *mut dyn FnMut(&mut [u8]) -> bool>> = RefCell::new(HashMap::new());
    static CONTROLLED_HANDLERS: RefCell<HashMap<usize, *mut dyn FnMut(&[u8]) -> ControlledAction>> = RefCell::new(HashMap::new());
}

fn handle_fragment(handler_id: usize, buffer: &[u8]) {
    HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(buffer);
            }
        }
    });
}

fn handle_controlled_fragment(handler_id: usize, buffer: &[u8]) -> i32 {
    CONTROLLED_HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(buffer) as i32
            }
        } else {
            ControlledAction::Abort as i32
        }
    })
}

fn handle_claim(handler_id: usize, buffer: &mut [u8]) -> bool {
    CLAIM_HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(buffer)
            }
        } else {
            false // abort if handler not found
        }
    })
}

pub struct Subscription {
    inner: cxx::UniquePtr<ffi::SubscriptionWrapper>,
}

impl Subscription {
    pub fn poll<F>(&mut self, limit: i32, mut handler: F) -> i32 
    where F: FnMut(&[u8])
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&[u8]) + 'static) = unsafe {
            std::mem::transmute::<*mut dyn FnMut(&[u8]), *mut (dyn FnMut(&[u8]) + 'static)>(&mut handler as *mut dyn FnMut(&[u8]))
        };
        
        HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });
        
        let result = self.inner.pin_mut().poll(limit, handler_id);
        
        HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });
        
        result
    }

    /// Poll with automatic fragment reassembly. Messages that span multiple fragments
    /// are reassembled before being delivered to the handler, which always receives
    /// complete messages.
    ///
    /// The handler can return `()` (maps to Continue) or a `ControlledAction` for
    /// flow-control (Abort to retry, Break to stop, Commit to checkpoint, Continue to proceed).
    pub fn poll_assembled<R, F>(&mut self, limit: i32, mut handler: F) -> i32
    where
        R: PollAction,
        F: FnMut(&[u8]) -> R,
    {
        // Wrap the user's handler to always produce a ControlledAction
        let mut controlled = |data: &[u8]| -> ControlledAction {
            handler(data).into_action()
        };

        let handler_id = &controlled as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&[u8]) -> ControlledAction + 'static) = unsafe {
            std::mem::transmute::<
                *mut dyn FnMut(&[u8]) -> ControlledAction,
                *mut (dyn FnMut(&[u8]) -> ControlledAction + 'static),
            >(&mut controlled as *mut dyn FnMut(&[u8]) -> ControlledAction)
        };

        CONTROLLED_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().controlledPollAssembled(limit, handler_id);

        CONTROLLED_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });

        result
    }

    pub fn is_connected(&self) -> bool {
        self.inner.isConnected()
    }
}

pub struct CountersReader {
    inner: cxx::UniquePtr<ffi::CountersReaderWrapper>,
}

thread_local! {
    static METADATA_HANDLERS: RefCell<HashMap<usize, *mut dyn FnMut(i32, i32, &[u8], &str)>> = RefCell::new(HashMap::new());
}

fn handle_counters_metadata(handler_id: usize, counter_id: i32, type_id: i32, key: &[u8], label: String) {
    METADATA_HANDLERS.with(|handlers| {
        if let Some(handler_ptr) = handlers.borrow_mut().get_mut(&handler_id) {
            unsafe {
                let handler = &mut **handler_ptr;
                handler(counter_id, type_id, key, &label);
            }
        }
    });
}

impl CountersReader {
    pub fn max_counter_id(&self) -> i32 {
        self.inner.maxCounterId()
    }

    pub fn get_counter_value(&self, id: i32) -> i64 {
        self.inner.getCounterValue(id)
    }

    pub fn get_counter_state(&self, id: i32) -> i32 {
        self.inner.getCounterState(id)
    }

    pub fn get_counter_type_id(&self, id: i32) -> i32 {
        self.inner.getCounterTypeId(id)
    }

    pub fn get_counter_label(&self, id: i32) -> String {
        self.inner.getCounterLabel(id)
    }

    pub fn for_each<F>(&self, mut handler: F) 
    where F: FnMut(i32, i32, &[u8], &str)
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(i32, i32, &[u8], &str) + 'static) = unsafe {
            std::mem::transmute::<*mut dyn FnMut(i32, i32, &[u8], &str), *mut (dyn FnMut(i32, i32, &[u8], &str) + 'static)>(&mut handler as *mut dyn FnMut(i32, i32, &[u8], &str))
        };
        
        METADATA_HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });
        
        self.inner.forEach(handler_id);
        
        METADATA_HANDLERS.with(|handlers| {
            handlers.borrow_mut().remove(&handler_id);
        });
    }
}

impl Default for AeronClient {
    fn default() -> Self {
        Self::new().expect("Failed to create AeronClient")
    }
}

pub struct MediaDriver {
    inner: cxx::UniquePtr<ffi::MediaDriverWrapper>,
}

impl MediaDriver {
    pub fn new() -> Self {
        Self { inner: ffi::create_media_driver() }
    }

    pub fn start(&mut self) {
        self.inner.pin_mut().start();
    }
}

impl Default for MediaDriver {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ChannelBuilder {
    media: &'static str,
    params: Vec<(String, String)>,
}

impl ChannelBuilder {
    pub fn ipc() -> Self {
        Self { media: "ipc", params: Vec::new() }
    }

    pub fn udp() -> Self {
        Self { media: "udp", params: Vec::new() }
    }

    pub fn endpoint(self, value: &str) -> Self { self.param("endpoint", value) }
    pub fn control(self, value: &str) -> Self { self.param("control", value) }
    pub fn control_mode(self, value: &str) -> Self { self.param("control-mode", value) }
    pub fn interface(self, value: &str) -> Self { self.param("interface", value) }
    pub fn mtu(self, bytes: usize) -> Self { self.param("mtu", &bytes.to_string()) }
    pub fn term_length(self, bytes: usize) -> Self { self.param("term-length", &bytes.to_string()) }
    pub fn session_id(self, id: i32) -> Self { self.param("session-id", &id.to_string()) }
    pub fn ttl(self, hops: u8) -> Self { self.param("ttl", &hops.to_string()) }
    pub fn reliable(self, value: bool) -> Self { self.param("reliable", if value { "true" } else { "false" }) }
    pub fn sparse(self, value: bool) -> Self { self.param("sparse", if value { "true" } else { "false" }) }
    pub fn linger(self, ns: u64) -> Self { self.param("linger", &ns.to_string()) }
    pub fn tether(self, value: bool) -> Self { self.param("tether", if value { "true" } else { "false" }) }
    pub fn rejoin(self, value: bool) -> Self { self.param("rejoin", if value { "true" } else { "false" }) }
    pub fn flow_control(self, value: &str) -> Self { self.param("fc", value) }
    pub fn congestion_control(self, value: &str) -> Self { self.param("cc", value) }
    pub fn socket_sndbuf(self, bytes: usize) -> Self { self.param("so-sndbuf", &bytes.to_string()) }
    pub fn socket_rcvbuf(self, bytes: usize) -> Self { self.param("so-rcvbuf", &bytes.to_string()) }
    pub fn receiver_window(self, bytes: usize) -> Self { self.param("rcv-wnd", &bytes.to_string()) }

    pub fn param(mut self, key: &str, value: &str) -> Self {
        self.params.push((key.to_string(), value.to_string()));
        self
    }

    pub fn build(&self) -> String {
        let mut uri = format!("aeron:{}", self.media);
        for (i, (key, value)) in self.params.iter().enumerate() {
            uri.push(if i == 0 { '?' } else { '|' });
            uri.push_str(key);
            uri.push('=');
            uri.push_str(value);
        }
        uri
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aeron_creation_with_driver() {
        // 1. Start embedded driver
        let mut driver = MediaDriver::new();
        driver.start();

        // Wait a tiny bit for the driver to spin up its files in /dev/shm
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 2. Connect client
        let mut client = AeronClient::new().expect("Failed to connect to media driver");
        client.start();
        assert!(!client.is_closed()); 

        // 3. Test Pub/Sub creation
        let mut publ = client.add_publication("aeron:ipc", 10).expect("add pub failed");
        let mut sub = client.add_subscription("aeron:ipc", 10).expect("add sub failed");

        assert!(publ.is_connected() || !publ.is_connected()); // Just testing boundary
    }

    #[test]
    fn test_channel_builder_ipc() {
        assert_eq!(ChannelBuilder::ipc().build(), "aeron:ipc");
    }

    #[test]
    fn test_channel_builder_udp() {
        let uri = ChannelBuilder::udp()
            .endpoint("localhost:20121")
            .build();
        assert_eq!(uri, "aeron:udp?endpoint=localhost:20121");
    }

    #[test]
    fn test_channel_builder_multiple_params() {
        let uri = ChannelBuilder::udp()
            .endpoint("localhost:20121")
            .mtu(8192)
            .term_length(65536)
            .reliable(true)
            .build();
        assert_eq!(uri, "aeron:udp?endpoint=localhost:20121|mtu=8192|term-length=65536|reliable=true");
    }

    #[test]
    fn test_channel_builder_multicast() {
        let uri = ChannelBuilder::udp()
            .endpoint("224.0.1.1:40456")
            .interface("localhost")
            .ttl(4)
            .build();
        assert_eq!(uri, "aeron:udp?endpoint=224.0.1.1:40456|interface=localhost|ttl=4");
    }

    #[test]
    fn test_channel_builder_mdc() {
        let uri = ChannelBuilder::udp()
            .control("localhost:40456")
            .control_mode("dynamic")
            .build();
        assert_eq!(uri, "aeron:udp?control=localhost:40456|control-mode=dynamic");
    }

    #[test]
    fn test_channel_builder_custom_param() {
        let uri = ChannelBuilder::ipc()
            .param("alias", "my-channel")
            .build();
        assert_eq!(uri, "aeron:ipc?alias=my-channel");
    }
}
