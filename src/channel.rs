//! Channel URIs ([`ChannelBuilder`]).

/// Builder for Aeron channel URIs (`aeron:ipc` or `aeron:udp?key=value|...`).
///
/// # Examples
///
/// ```
/// use aeron_glide::ChannelBuilder;
///
/// let ipc = ChannelBuilder::ipc().build();
/// assert_eq!(ipc, "aeron:ipc");
///
/// let udp = ChannelBuilder::udp()
///     .endpoint("localhost:20121")
///     .mtu(8192)
///     .build();
/// assert_eq!(udp, "aeron:udp?endpoint=localhost:20121|mtu=8192");
/// ```
pub struct ChannelBuilder {
    media: &'static str,
    params: Vec<(String, String)>,
}

impl ChannelBuilder {
    /// Create an IPC (shared memory) channel builder.
    pub fn ipc() -> Self {
        Self {
            media: "ipc",
            params: Vec::new(),
        }
    }

    /// Create a UDP channel builder.
    pub fn udp() -> Self {
        Self {
            media: "udp",
            params: Vec::new(),
        }
    }

    /// Set the endpoint address (e.g., `"localhost:20121"` or `"224.0.1.1:40456"` for multicast).
    pub fn endpoint(self, value: &str) -> Self {
        self.param("endpoint", value)
    }
    pub fn control(self, value: &str) -> Self {
        self.param("control", value)
    }
    pub fn control_mode(self, value: &str) -> Self {
        self.param("control-mode", value)
    }
    pub fn interface(self, value: &str) -> Self {
        self.param("interface", value)
    }
    pub fn mtu(self, bytes: usize) -> Self {
        self.param("mtu", &bytes.to_string())
    }
    pub fn term_length(self, bytes: usize) -> Self {
        self.param("term-length", &bytes.to_string())
    }
    pub fn session_id(self, id: i32) -> Self {
        self.param("session-id", &id.to_string())
    }
    pub fn ttl(self, hops: u8) -> Self {
        self.param("ttl", &hops.to_string())
    }
    pub fn reliable(self, value: bool) -> Self {
        self.param("reliable", if value { "true" } else { "false" })
    }
    pub fn sparse(self, value: bool) -> Self {
        self.param("sparse", if value { "true" } else { "false" })
    }
    pub fn linger(self, ns: u64) -> Self {
        self.param("linger", &ns.to_string())
    }
    pub fn tether(self, value: bool) -> Self {
        self.param("tether", if value { "true" } else { "false" })
    }
    pub fn rejoin(self, value: bool) -> Self {
        self.param("rejoin", if value { "true" } else { "false" })
    }
    pub fn flow_control(self, value: &str) -> Self {
        self.param("fc", value)
    }
    pub fn congestion_control(self, value: &str) -> Self {
        self.param("cc", value)
    }
    pub fn socket_sndbuf(self, bytes: usize) -> Self {
        self.param("so-sndbuf", &bytes.to_string())
    }
    pub fn socket_rcvbuf(self, bytes: usize) -> Self {
        self.param("so-rcvbuf", &bytes.to_string())
    }
    pub fn receiver_window(self, bytes: usize) -> Self {
        self.param("rcv-wnd", &bytes.to_string())
    }

    /// Set an arbitrary channel parameter by key and value.
    pub fn param(mut self, key: &str, value: &str) -> Self {
        self.params.push((key.to_string(), value.to_string()));
        self
    }

    /// Build the channel URI string.
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
