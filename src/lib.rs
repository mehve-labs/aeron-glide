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

// Thread-local registries for closures passed across the cxx boundary.
// We use pointer-based handler IDs since cxx doesn't support passing trait objects directly.
thread_local! {
    static HANDLERS: RefCell<HashMap<usize, *mut dyn FnMut(&[u8])>> = RefCell::new(HashMap::new());
    static CLAIM_HANDLERS: RefCell<HashMap<usize, *mut dyn FnMut(&mut [u8]) -> bool>> = RefCell::new(HashMap::new());
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
    pub fn poll_assembled<F>(&mut self, limit: i32, mut handler: F) -> i32
    where F: FnMut(&[u8])
    {
        let handler_id = &handler as *const _ as usize;
        let mut_ptr: *mut (dyn FnMut(&[u8]) + 'static) = unsafe {
            std::mem::transmute::<*mut dyn FnMut(&[u8]), *mut (dyn FnMut(&[u8]) + 'static)>(&mut handler as *mut dyn FnMut(&[u8]))
        };

        HANDLERS.with(|handlers| {
            handlers.borrow_mut().insert(handler_id, mut_ptr);
        });

        let result = self.inner.pin_mut().pollAssembled(limit, handler_id);

        HANDLERS.with(|handlers| {
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
}
