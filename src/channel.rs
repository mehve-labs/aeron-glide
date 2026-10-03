//! Channel URIs: building ([`ChannelBuilder`]) and parsing ([`ChannelUri`]).
//!
//! These port the C++ `aeron::ChannelUriStringBuilder` and `aeron::ChannelUri`
//! (header-only string handling, with no client state) to Rust, keeping their
//! parameter names, output order and validation. The parameter name constants
//! are the C++ ones.
//!
//! # Response channels
//!
//! A response channel lets a server answer each client on a channel the client
//! controls, without knowing the client's address up front (`control-mode=response`):
//!
//! ```no_run
//! use aeron_glide::{AeronClient, ChannelBuilder, ControlMode};
//!
//! # fn main() -> aeron_glide::Result<()> {
//! let client = AeronClient::new()?;
//! // Server: a plain subscription for requests.
//! let requests = client.add_subscription("aeron:udp?endpoint=localhost:10001", 1)?;
//!
//! // Client: a response subscription, then a request publication tagged with its
//! // registration ID.
//! let responses = client.add_subscription(
//!     &ChannelBuilder::udp()
//!         .control_mode(ControlMode::Response)
//!         .control_endpoint("localhost:10002")
//!         .build()?,
//!     2,
//! )?;
//! let request_publication = client.add_publication(
//!     &ChannelBuilder::udp()
//!         .endpoint("localhost:10001")
//!         .response_correlation_id(responses.registration_id())
//!         .build()?,
//!     1,
//! )?;
//!
//! // Server, for each request image: a response publication tagged with the
//! // image's correlation ID.
//! # let image = requests.images().into_iter().next().unwrap();
//! let response_publication = client.add_publication(
//!     &ChannelBuilder::udp()
//!         .control_mode(ControlMode::Response)
//!         .control_endpoint("localhost:10002")
//!         .response_correlation_id(image.correlation_id())
//!         .build()?,
//!     2,
//! )?;
//! # Ok(())
//! # }
//! ```

use crate::{Error, ErrorKind, Result};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// Qualifier of a spy subscription's prefix (`aeron-spy:aeron:...`).
pub const SPY_QUALIFIER: &str = "aeron-spy";
/// The URI scheme of every Aeron channel.
pub const AERON_SCHEME: &str = "aeron";
/// `"aeron:"`.
pub const AERON_PREFIX: &str = "aeron:";
/// The IPC (shared memory) media.
pub const IPC_MEDIA: &str = "ipc";
/// The UDP media.
pub const UDP_MEDIA: &str = "udp";
/// The IPC channel, `"aeron:ipc"`.
pub const IPC_CHANNEL: &str = "aeron:ipc";
/// `"aeron-spy:"`.
pub const SPY_PREFIX: &str = "aeron-spy:";
/// The maximum length of a channel URI (Java `ChannelUri.MAX_URI_LENGTH`; the
/// media driver rejects longer ones).
pub const MAX_URI_LENGTH: usize = 4095;
/// Prefix of a tag reference in a value (e.g. `session-id=tag:5`).
pub const TAG_PREFIX: &str = "tag:";

/// `endpoint`: the address of a UDP channel.
pub const ENDPOINT_PARAM_NAME: &str = "endpoint";
/// `interface`: the network interface to use.
pub const INTERFACE_PARAM_NAME: &str = "interface";
/// `init-term-id`.
pub const INITIAL_TERM_ID_PARAM_NAME: &str = "init-term-id";
/// `term-id`.
pub const TERM_ID_PARAM_NAME: &str = "term-id";
/// `term-offset`.
pub const TERM_OFFSET_PARAM_NAME: &str = "term-offset";
/// `term-length`.
pub const TERM_LENGTH_PARAM_NAME: &str = "term-length";
/// `mtu`.
pub const MTU_LENGTH_PARAM_NAME: &str = "mtu";
/// `ttl`: multicast time to live.
pub const TTL_PARAM_NAME: &str = "ttl";
/// `control`: the control address of a multi-destination or response channel.
pub const MDC_CONTROL_PARAM_NAME: &str = "control";
/// `control-mode`.
pub const MDC_CONTROL_MODE_PARAM_NAME: &str = "control-mode";
/// `control-mode=manual`.
pub const MDC_CONTROL_MODE_MANUAL: &str = "manual";
/// `control-mode=dynamic`.
pub const MDC_CONTROL_MODE_DYNAMIC: &str = "dynamic";
/// `control-mode=response`.
pub const CONTROL_MODE_RESPONSE: &str = "response";
/// `session-id`.
pub const SESSION_ID_PARAM_NAME: &str = "session-id";
/// `linger` (nanoseconds).
pub const LINGER_PARAM_NAME: &str = "linger";
/// `reliable`.
pub const RELIABLE_STREAM_PARAM_NAME: &str = "reliable";
/// `tags`.
pub const TAGS_PARAM_NAME: &str = "tags";
/// `sparse`.
pub const SPARSE_PARAM_NAME: &str = "sparse";
/// `alias`.
pub const ALIAS_PARAM_NAME: &str = "alias";
/// `eos`: send an end-of-stream when the publication closes.
pub const EOS_PARAM_NAME: &str = "eos";
/// `tether`.
pub const TETHER_PARAM_NAME: &str = "tether";
/// `group`.
pub const GROUP_PARAM_NAME: &str = "group";
/// `rejoin`.
pub const REJOIN_PARAM_NAME: &str = "rejoin";
/// `cc`: congestion control.
pub const CONGESTION_CONTROL_PARAM_NAME: &str = "cc";
/// `fc`: flow control.
pub const FLOW_CONTROL_PARAM_NAME: &str = "fc";
/// `gtag`: group tag.
pub const GROUP_TAG_PARAM_NAME: &str = "gtag";
/// `ssc`: spies simulate connection.
pub const SPIES_SIMULATE_CONNECTION_PARAM_NAME: &str = "ssc";
/// `so-sndbuf`.
pub const SOCKET_SNDBUF_PARAM_NAME: &str = "so-sndbuf";
/// `so-rcvbuf`.
pub const SOCKET_RCVBUF_PARAM_NAME: &str = "so-rcvbuf";
/// `rcv-wnd`.
pub const RECEIVER_WINDOW_LENGTH_PARAM_NAME: &str = "rcv-wnd";
/// `media-rcv-ts-offset`.
pub const MEDIA_RCV_TIMESTAMP_OFFSET_PARAM_NAME: &str = "media-rcv-ts-offset";
/// `channel-rcv-ts-offset`.
pub const CHANNEL_RCV_TIMESTAMP_OFFSET_PARAM_NAME: &str = "channel-rcv-ts-offset";
/// `channel-snd-ts-offset`.
pub const CHANNEL_SND_TIMESTAMP_OFFSET_PARAM_NAME: &str = "channel-snd-ts-offset";
/// `response-correlation-id`.
pub const RESPONSE_CORRELATION_ID_PARAM_NAME: &str = "response-correlation-id";
/// `nak-delay` (nanoseconds).
pub const NAK_DELAY_PARAM_NAME: &str = "nak-delay";
/// `untethered-window-limit-timeout` (nanoseconds).
pub const UNTETHERED_WINDOW_LIMIT_TIMEOUT_PARAM_NAME: &str = "untethered-window-limit-timeout";
/// `untethered-resting-timeout` (nanoseconds).
pub const UNTETHERED_RESTING_TIMEOUT_PARAM_NAME: &str = "untethered-resting-timeout";
/// `max-resend`.
pub const MAX_RESEND_PARAM_NAME: &str = "max-resend";

/// The order in which C++ `ChannelUriStringBuilder::build` writes parameters.
const BUILD_ORDER: [&str; 35] = [
    TAGS_PARAM_NAME,
    ENDPOINT_PARAM_NAME,
    INTERFACE_PARAM_NAME,
    MDC_CONTROL_PARAM_NAME,
    MDC_CONTROL_MODE_PARAM_NAME,
    MTU_LENGTH_PARAM_NAME,
    TERM_LENGTH_PARAM_NAME,
    INITIAL_TERM_ID_PARAM_NAME,
    TERM_ID_PARAM_NAME,
    TERM_OFFSET_PARAM_NAME,
    SESSION_ID_PARAM_NAME,
    TTL_PARAM_NAME,
    RELIABLE_STREAM_PARAM_NAME,
    LINGER_PARAM_NAME,
    ALIAS_PARAM_NAME,
    CONGESTION_CONTROL_PARAM_NAME,
    FLOW_CONTROL_PARAM_NAME,
    GROUP_TAG_PARAM_NAME,
    SPARSE_PARAM_NAME,
    EOS_PARAM_NAME,
    TETHER_PARAM_NAME,
    GROUP_PARAM_NAME,
    REJOIN_PARAM_NAME,
    SPIES_SIMULATE_CONNECTION_PARAM_NAME,
    SOCKET_SNDBUF_PARAM_NAME,
    SOCKET_RCVBUF_PARAM_NAME,
    RECEIVER_WINDOW_LENGTH_PARAM_NAME,
    MEDIA_RCV_TIMESTAMP_OFFSET_PARAM_NAME,
    CHANNEL_RCV_TIMESTAMP_OFFSET_PARAM_NAME,
    CHANNEL_SND_TIMESTAMP_OFFSET_PARAM_NAME,
    RESPONSE_CORRELATION_ID_PARAM_NAME,
    NAK_DELAY_PARAM_NAME,
    UNTETHERED_WINDOW_LIMIT_TIMEOUT_PARAM_NAME,
    UNTETHERED_RESTING_TIMEOUT_PARAM_NAME,
    MAX_RESEND_PARAM_NAME,
];

/// Frame alignment: MTUs, term offsets and positions are multiples of it.
const FRAME_ALIGNMENT: u32 = 32;
const TERM_MIN_LENGTH: u32 = 64 * 1024;
const TERM_MAX_LENGTH: u32 = 1024 * 1024 * 1024;

/// The `control-mode` of a UDP channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ControlMode {
    /// Multi-destination-cast with destinations added by the publisher
    /// (`Publication::add_destination`).
    Manual,
    /// Multi-destination-cast with destinations added as subscribers send
    /// status messages to the control address.
    Dynamic,
    /// A response channel (see the [module docs](crate::channel)).
    Response,
}

impl ControlMode {
    /// The URI value (`manual`, `dynamic` or `response`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => MDC_CONTROL_MODE_MANUAL,
            Self::Dynamic => MDC_CONTROL_MODE_DYNAMIC,
            Self::Response => CONTROL_MODE_RESPONSE,
        }
    }
}

impl FromStr for ControlMode {
    type Err = Error;

    fn from_str(mode: &str) -> Result<Self> {
        match mode {
            MDC_CONTROL_MODE_MANUAL => Ok(Self::Manual),
            MDC_CONTROL_MODE_DYNAMIC => Ok(Self::Dynamic),
            CONTROL_MODE_RESPONSE => Ok(Self::Response),
            _ => Err(einval(
                ErrorKind::IllegalArgument,
                format!("invalid control mode: {mode}"),
            )),
        }
    }
}

impl fmt::Display for ControlMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Builder for Aeron channel URIs (C++ `aeron::ChannelUriStringBuilder`).
///
/// Parameters are written in the C++ builder's order, followed by any set with
/// [`param`](Self::param) in the order they were added; setting a parameter
/// again replaces it. Invalid values (e.g. an MTU that is not a multiple of 32)
/// are reported by [`build`](Self::build), which fails with the first one.
///
/// # Examples
///
/// ```
/// use aeron_glide::{ChannelBuilder, ControlMode};
///
/// let ipc = ChannelBuilder::ipc().build()?;
/// assert_eq!(ipc, "aeron:ipc");
///
/// let udp = ChannelBuilder::udp()
///     .mtu(8192)
///     .endpoint("localhost:20121")
///     .build()?;
/// assert_eq!(udp, "aeron:udp?endpoint=localhost:20121|mtu=8192");
///
/// let mdc = ChannelBuilder::udp()
///     .control_endpoint("localhost:40456")
///     .control_mode(ControlMode::Dynamic)
///     .build()?;
/// assert_eq!(mdc, "aeron:udp?control=localhost:40456|control-mode=dynamic");
///
/// assert!(ChannelBuilder::udp().mtu(1000).build().is_err());
/// # Ok::<(), aeron_glide::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct ChannelBuilder {
    prefix: Option<String>,
    media: String,
    params: Vec<(String, String)>,
    session_id: Option<i32>,
    session_id_tagged: bool,
    error: Option<Error>,
}

impl ChannelBuilder {
    /// A builder for an IPC (shared memory) channel.
    pub fn ipc() -> Self {
        Self::with_media(IPC_MEDIA)
    }

    /// A builder for a UDP channel.
    pub fn udp() -> Self {
        Self::with_media(UDP_MEDIA)
    }

    fn with_media(media: &str) -> Self {
        Self {
            prefix: None,
            media: media.to_string(),
            params: Vec::new(),
            session_id: None,
            session_id_tagged: false,
            error: None,
        }
    }

    /// Remove every parameter and the prefix (C++ `clear`). The media is kept,
    /// and so is an invalid setting made before (C++ would have thrown it).
    pub fn clear(mut self) -> Self {
        let media = std::mem::take(&mut self.media);
        let error = self.error.take();
        Self {
            error,
            ..Self::with_media(&media)
        }
    }

    /// Set the prefix: [`SPY_QUALIFIER`] (`"aeron-spy"`) for a spy subscription,
    /// or `""` for none.
    pub fn prefix(mut self, prefix: &str) -> Self {
        if !prefix.is_empty() && prefix != SPY_QUALIFIER {
            return self.fail(
                ErrorKind::IllegalArgument,
                format!("invalid prefix: {prefix}"),
            );
        }
        self.prefix = Some(prefix.to_string());
        self
    }

    /// Set the media: [`IPC_MEDIA`] or [`UDP_MEDIA`].
    pub fn media(mut self, media: &str) -> Self {
        if media != IPC_MEDIA && media != UDP_MEDIA {
            return self.fail(
                ErrorKind::IllegalArgument,
                format!("invalid media: {media}"),
            );
        }
        self.media = media.to_string();
        self
    }

    /// The UDP address (`host:port`) to publish to or subscribe on; a multicast
    /// address for multicast.
    pub fn endpoint(self, endpoint: &str) -> Self {
        self.param(ENDPOINT_PARAM_NAME, endpoint)
    }

    /// The network interface to send or receive on (`interface`), e.g.
    /// `"192.168.1.0/24"`.
    pub fn network_interface(self, interface: &str) -> Self {
        self.param(INTERFACE_PARAM_NAME, interface)
    }

    /// The control address of a multi-destination-cast or response channel
    /// (`control`).
    pub fn control_endpoint(self, endpoint: &str) -> Self {
        self.param(MDC_CONTROL_PARAM_NAME, endpoint)
    }

    /// The control mode (`control-mode`).
    pub fn control_mode(self, mode: ControlMode) -> Self {
        self.param(MDC_CONTROL_MODE_PARAM_NAME, mode.as_str())
    }

    /// Comma-separated tags (`tags`), e.g. `"1001"` or `"1001,1002"`, so other
    /// channels can refer to this one with `tag:1001`.
    pub fn tags(self, tags: &str) -> Self {
        self.param(TAGS_PARAM_NAME, tags)
    }

    /// An alias shown in tools and counter labels (`alias`).
    pub fn alias(self, alias: &str) -> Self {
        self.param(ALIAS_PARAM_NAME, alias)
    }

    /// The congestion control algorithm (`cc`), e.g. `"static"` or `"cubic"`.
    pub fn congestion_control(self, congestion_control: &str) -> Self {
        self.param(CONGESTION_CONTROL_PARAM_NAME, congestion_control)
    }

    /// The flow control strategy (`fc`), e.g. `"max"`, `"min"`,
    /// `"min,t:5s"` or `"tagged,g:1001/3"`.
    pub fn flow_control(self, flow_control: &str) -> Self {
        self.param(FLOW_CONTROL_PARAM_NAME, flow_control)
    }

    /// The group tag of receivers, for tagged flow control (`gtag`).
    pub fn group_tag(self, group_tag: i64) -> Self {
        self.param(GROUP_TAG_PARAM_NAME, &group_tag.to_string())
    }

    /// Whether lost data is recovered (`reliable`); `false` fills gaps instead.
    pub fn reliable(self, reliable: bool) -> Self {
        self.bool_param(RELIABLE_STREAM_PARAM_NAME, reliable)
    }

    /// The multicast time to live (`ttl`).
    pub fn ttl(self, ttl: u8) -> Self {
        self.param(TTL_PARAM_NAME, &ttl.to_string())
    }

    /// The maximum transmission unit (`mtu`): 32 to 65504 bytes, a multiple of 32.
    pub fn mtu(self, mtu: u32) -> Self {
        if !(32..=65504).contains(&mtu) {
            return self.fail(
                ErrorKind::IllegalArgument,
                format!("MTU not in range 32-65504: {mtu}"),
            );
        }
        if !mtu.is_multiple_of(FRAME_ALIGNMENT) {
            return self.fail(
                ErrorKind::IllegalArgument,
                format!("MTU not a multiple of FRAME_ALIGNMENT: mtu={mtu}"),
            );
        }
        self.param(MTU_LENGTH_PARAM_NAME, &mtu.to_string())
    }

    /// The term buffer length (`term-length`): a power of two from 64 KiB to 1 GiB.
    pub fn term_length(self, term_length: u32) -> Self {
        match check_term_length(term_length) {
            Ok(()) => self.param(TERM_LENGTH_PARAM_NAME, &term_length.to_string()),
            Err(e) => self.fail_with(e),
        }
    }

    /// The initial term ID of a publication (`init-term-id`).
    pub fn initial_term_id(self, initial_term_id: i32) -> Self {
        self.param(INITIAL_TERM_ID_PARAM_NAME, &initial_term_id.to_string())
    }

    /// The term ID a publication starts at (`term-id`).
    pub fn term_id(self, term_id: i32) -> Self {
        self.param(TERM_ID_PARAM_NAME, &term_id.to_string())
    }

    /// The offset in the term a publication starts at (`term-offset`): at most
    /// 1 GiB, a multiple of 32.
    pub fn term_offset(self, term_offset: u32) -> Self {
        if term_offset > TERM_MAX_LENGTH {
            return self.fail(
                ErrorKind::IllegalArgument,
                format!("term offset not in range 0-1g: {term_offset}"),
            );
        }
        if !term_offset.is_multiple_of(FRAME_ALIGNMENT) {
            return self.fail(
                ErrorKind::IllegalArgument,
                format!("term offset not multiple of FRAME_ALIGNMENT: {term_offset}"),
            );
        }
        self.param(TERM_OFFSET_PARAM_NAME, &term_offset.to_string())
    }

    /// Set `init-term-id`, `term-id`, `term-offset` and `term-length` so a
    /// publication starts at `position` (C++ `initialPosition`), e.g. to resume a
    /// stream. `position` must be non-negative and a multiple of 32. The term ID
    /// wraps around like other term IDs (as in Java; C++ writes a value out of
    /// the `i32` range the driver rejects).
    pub fn initial_position(self, position: i64, initial_term_id: i32, term_length: u32) -> Self {
        if position < 0 || position % i64::from(FRAME_ALIGNMENT) != 0 {
            return self.fail(
                ErrorKind::IllegalArgument,
                format!("position not multiple of FRAME_ALIGNMENT: {position}"),
            );
        }
        if let Err(e) = check_term_length(term_length) {
            return self.fail_with(e);
        }
        let bits_to_shift = term_length.trailing_zeros();
        // Wraps like Java's computeTermIdFromPosition. C++ writes the 64-bit sum,
        // which the driver rejects as out of range when it overflows an i32.
        let term_id = ((position >> bits_to_shift) as i32).wrapping_add(initial_term_id);
        let term_offset = position & (i64::from(term_length) - 1);
        self.param(TERM_LENGTH_PARAM_NAME, &term_length.to_string())
            .param(INITIAL_TERM_ID_PARAM_NAME, &initial_term_id.to_string())
            .param(TERM_ID_PARAM_NAME, &term_id.to_string())
            .param(TERM_OFFSET_PARAM_NAME, &term_offset.to_string())
    }

    /// The session ID of a publication, or of the images a subscription accepts
    /// (`session-id`).
    pub fn session_id(mut self, session_id: i32) -> Self {
        self.remove_param(SESSION_ID_PARAM_NAME);
        self.session_id = Some(session_id);
        self
    }

    /// Write the session ID as a tag reference (`session-id=tag:<id>`), i.e. the
    /// session ID of the publication tagged `<id>` (C++ `isSessionIdTagged`).
    pub fn session_id_tagged(mut self, tagged: bool) -> Self {
        self.session_id_tagged = tagged;
        self
    }

    /// How long a closed publication lingers to let subscribers catch up
    /// (`linger`, written in nanoseconds).
    pub fn linger(self, linger: Duration) -> Self {
        self.duration_param(LINGER_PARAM_NAME, linger)
    }

    /// Whether a publication's log buffers are sparse files (`sparse`).
    pub fn sparse(self, sparse: bool) -> Self {
        self.bool_param(SPARSE_PARAM_NAME, sparse)
    }

    /// Whether a publication sends an end-of-stream when it closes (`eos`).
    pub fn eos(self, eos: bool) -> Self {
        self.bool_param(EOS_PARAM_NAME, eos)
    }

    /// Whether a subscription is tethered: slow tethered subscribers hold the
    /// publisher back; untethered ones are dropped (`tether`).
    pub fn tether(self, tether: bool) -> Self {
        self.bool_param(TETHER_PARAM_NAME, tether)
    }

    /// Whether a subscription takes part in group semantics, e.g. multicast
    /// (`group`).
    pub fn group(self, group: bool) -> Self {
        self.bool_param(GROUP_PARAM_NAME, group)
    }

    /// Whether a subscription rejoins a stream after losing its image (`rejoin`).
    pub fn rejoin(self, rejoin: bool) -> Self {
        self.bool_param(REJOIN_PARAM_NAME, rejoin)
    }

    /// Whether spy subscriptions count as connected subscribers (`ssc`).
    pub fn spies_simulate_connection(self, value: bool) -> Self {
        self.bool_param(SPIES_SIMULATE_CONNECTION_PARAM_NAME, value)
    }

    /// The socket send buffer length (`so-sndbuf`).
    pub fn socket_sndbuf_length(self, length: u32) -> Self {
        self.param(SOCKET_SNDBUF_PARAM_NAME, &length.to_string())
    }

    /// The socket receive buffer length (`so-rcvbuf`).
    pub fn socket_rcvbuf_length(self, length: u32) -> Self {
        self.param(SOCKET_RCVBUF_PARAM_NAME, &length.to_string())
    }

    /// The receiver window length (`rcv-wnd`).
    pub fn receiver_window_length(self, length: u32) -> Self {
        self.param(RECEIVER_WINDOW_LENGTH_PARAM_NAME, &length.to_string())
    }

    /// Where to write the media receive timestamp in received frames
    /// (`media-rcv-ts-offset`): `"reserved"` (the reserved value) or a byte offset.
    pub fn media_receive_timestamp_offset(self, offset: &str) -> Self {
        self.param(MEDIA_RCV_TIMESTAMP_OFFSET_PARAM_NAME, offset)
    }

    /// Where to write the channel receive timestamp in received frames
    /// (`channel-rcv-ts-offset`): `"reserved"` or a byte offset.
    pub fn channel_receive_timestamp_offset(self, offset: &str) -> Self {
        self.param(CHANNEL_RCV_TIMESTAMP_OFFSET_PARAM_NAME, offset)
    }

    /// Where to write the channel send timestamp in sent frames
    /// (`channel-snd-ts-offset`): `"reserved"` or a byte offset.
    pub fn channel_send_timestamp_offset(self, offset: &str) -> Self {
        self.param(CHANNEL_SND_TIMESTAMP_OFFSET_PARAM_NAME, offset)
    }

    /// The correlation ID a response channel answers
    /// (`response-correlation-id`): the response subscription's registration ID
    /// on a request publication, the request image's correlation ID on a response
    /// publication. See the [module docs](crate::channel).
    pub fn response_correlation_id(self, correlation_id: i64) -> Self {
        self.param(
            RESPONSE_CORRELATION_ID_PARAM_NAME,
            &correlation_id.to_string(),
        )
    }

    /// How long a receiver waits before sending a NAK for lost data
    /// (`nak-delay`, written in nanoseconds).
    pub fn nak_delay(self, delay: Duration) -> Self {
        self.duration_param(NAK_DELAY_PARAM_NAME, delay)
    }

    /// How long an untethered subscriber may hold the publisher at its window
    /// limit before it is dropped (`untethered-window-limit-timeout`, written in
    /// nanoseconds).
    pub fn untethered_window_limit_timeout(self, timeout: Duration) -> Self {
        self.duration_param(UNTETHERED_WINDOW_LIMIT_TIMEOUT_PARAM_NAME, timeout)
    }

    /// How long a dropped untethered subscriber rests before rejoining
    /// (`untethered-resting-timeout`, written in nanoseconds).
    pub fn untethered_resting_timeout(self, timeout: Duration) -> Self {
        self.duration_param(UNTETHERED_RESTING_TIMEOUT_PARAM_NAME, timeout)
    }

    /// The maximum number of NAK-driven retransmissions in flight
    /// (`max-resend`).
    pub fn max_resend(self, max_resend: i32) -> Self {
        self.param(MAX_RESEND_PARAM_NAME, &max_resend.to_string())
    }

    /// Set any parameter by name, e.g. one this builder has no setter for.
    /// Replaces a value set by a typed setter. A `session-id` set here is written
    /// as given, ignoring [`session_id_tagged`](Self::session_id_tagged).
    pub fn param(mut self, key: &str, value: &str) -> Self {
        if key == SESSION_ID_PARAM_NAME {
            self.session_id = None;
        }
        match self.params.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value.to_string(),
            None => self.params.push((key.to_string(), value.to_string())),
        }
        self
    }

    /// Unset a parameter (the C++ setters' `nullptr` overloads).
    pub fn remove(mut self, key: &str) -> Self {
        self.remove_param(key);
        if key == SESSION_ID_PARAM_NAME {
            self.session_id = None;
        }
        self
    }

    /// Build the channel URI, or fail with the first invalid setting.
    ///
    /// Also fails, unlike C++, if a parameter name is empty or contains `|`, `=`
    /// or `?`, a value is empty or contains `|`, or the URI is longer than
    /// [`MAX_URI_LENGTH`] (as Java): the media driver would reject the URI (or,
    /// for a trailing empty value such as `tags=`, crash).
    pub fn build(&self) -> Result<String> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let mut uri = String::new();
        if let Some(prefix) = self.prefix.as_deref().filter(|p| !p.is_empty()) {
            uri.push_str(prefix);
            uri.push(':');
        }
        uri.push_str(AERON_PREFIX);
        uri.push_str(&self.media);

        let session_id = self.session_id.map(|id| {
            if self.session_id_tagged {
                format!("{TAG_PREFIX}{id}")
            } else {
                id.to_string()
            }
        });
        let known = BUILD_ORDER.iter().filter_map(|&name| {
            if name == SESSION_ID_PARAM_NAME
                && let Some(id) = &session_id
            {
                return Some((name, id.as_str()));
            }
            self.get(name).map(|value| (name, value))
        });
        let custom = self
            .params
            .iter()
            .filter(|(k, _)| !BUILD_ORDER.contains(&k.as_str()))
            .map(|(k, v)| (k.as_str(), v.as_str()));
        for (i, (key, value)) in known.chain(custom).enumerate() {
            if key.is_empty() || key.contains(['|', '=', '?']) {
                return Err(einval(
                    ErrorKind::IllegalArgument,
                    format!("invalid channel parameter name: {key:?}"),
                ));
            }
            if value.is_empty() || value.contains('|') {
                return Err(einval(
                    ErrorKind::IllegalArgument,
                    format!("invalid value for channel parameter {key}: {value:?}"),
                ));
            }
            uri.push(if i == 0 { '?' } else { '|' });
            uri.push_str(key);
            uri.push('=');
            uri.push_str(value);
        }
        if uri.len() > MAX_URI_LENGTH {
            return Err(einval(
                ErrorKind::IllegalArgument,
                format!(
                    "URI length ({}) exceeds max supported length ({MAX_URI_LENGTH})",
                    uri.len()
                ),
            ));
        }
        Ok(uri)
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn remove_param(&mut self, key: &str) {
        self.params.retain(|(k, _)| k != key);
    }

    fn bool_param(self, key: &str, value: bool) -> Self {
        self.param(key, if value { "true" } else { "false" })
    }

    fn duration_param(self, key: &str, value: Duration) -> Self {
        match i64::try_from(value.as_nanos()) {
            Ok(ns) => self.param(key, &ns.to_string()),
            Err(_) => self.fail(
                ErrorKind::IllegalArgument,
                format!("{key} too large: {value:?}"),
            ),
        }
    }

    fn fail(self, kind: ErrorKind, message: String) -> Self {
        self.fail_with(einval(kind, message))
    }

    fn fail_with(mut self, error: Error) -> Self {
        self.error.get_or_insert(error);
        self
    }
}

fn check_term_length(term_length: u32) -> Result<()> {
    let message = if term_length < TERM_MIN_LENGTH {
        format!("term length less than min size of {TERM_MIN_LENGTH}, length={term_length}")
    } else if term_length > TERM_MAX_LENGTH {
        format!("term length greater than max size of {TERM_MAX_LENGTH}, length={term_length}")
    } else if !term_length.is_power_of_two() {
        format!("term length not a power of 2, length={term_length}")
    } else {
        return Ok(());
    };
    Err(einval(ErrorKind::IllegalState, message))
}

/// A parsed channel URI (C++ `aeron::ChannelUri`): prefix, media and
/// parameters, which can be read, changed and written back with
/// [`to_string`](ToString::to_string).
///
/// Unlike C++, parameters keep the order they were parsed or added in.
/// Equality ignores that order, as in Java.
///
/// ```
/// use aeron_glide::ChannelUri;
///
/// let mut uri: ChannelUri = "aeron:udp?endpoint=localhost:20121|mtu=1408".parse()?;
/// assert_eq!(uri.media(), "udp");
/// assert_eq!(uri.get("mtu"), Some("1408"));
/// uri.put("alias", "orders");
/// uri.remove("mtu");
/// assert_eq!(uri.to_string(), "aeron:udp?endpoint=localhost:20121|alias=orders");
/// # Ok::<(), aeron_glide::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct ChannelUri {
    prefix: String,
    media: String,
    params: Vec<(String, String)>,
}

impl ChannelUri {
    /// Parse a channel URI, e.g. `aeron:udp?endpoint=localhost:20121` or
    /// `aeron-spy:aeron:ipc`. A repeated parameter keeps its first value, as in
    /// C++ (Java and, for some parameters such as `endpoint`, the media driver
    /// keep the last one). Empty values are accepted, as in C++, although the
    /// driver rejects them.
    pub fn parse(uri: &str) -> Result<Self> {
        enum State {
            Media,
            Key,
            Value,
        }
        let (prefix, rest) = match uri.strip_prefix(SPY_PREFIX) {
            Some(rest) => (SPY_QUALIFIER, rest),
            None => ("", uri),
        };
        let Some(rest) = rest.strip_prefix(AERON_PREFIX) else {
            return Err(einval(
                ErrorKind::IllegalArgument,
                format!("Aeron URIs must start with 'aeron:', found: {uri}"),
            ));
        };
        let offset = uri.len() - rest.len();
        let mut params: Vec<(String, String)> = Vec::new();
        let mut media = String::new();
        let mut key = String::new();
        let mut builder = String::new();
        let mut state = State::Media;
        let add = |params: &mut Vec<(String, String)>, key: &str, value: String| {
            if !params.iter().any(|(k, _)| k == key) {
                params.push((key.to_string(), value));
            }
        };
        for (i, c) in rest.char_indices() {
            let index = offset + i;
            match state {
                State::Media => match c {
                    '?' => {
                        media = std::mem::take(&mut builder);
                        state = State::Key;
                    }
                    ':' | '|' | '=' => {
                        return Err(einval(
                            ErrorKind::IllegalState,
                            format!(
                                "encountered '{c}' within media definition at index {index} in {uri}"
                            ),
                        ));
                    }
                    _ => builder.push(c),
                },
                State::Key => match c {
                    '=' if builder.is_empty() => {
                        return Err(einval(
                            ErrorKind::IllegalState,
                            format!("empty key not allowed at index {index} in {uri}"),
                        ));
                    }
                    '=' => {
                        key = std::mem::take(&mut builder);
                        state = State::Value;
                    }
                    '|' => {
                        return Err(einval(
                            ErrorKind::IllegalState,
                            format!("invalid end of key at index {index} in {uri}"),
                        ));
                    }
                    _ => builder.push(c),
                },
                State::Value => match c {
                    '|' => {
                        add(&mut params, &key, std::mem::take(&mut builder));
                        state = State::Key;
                    }
                    _ => builder.push(c),
                },
            }
        }
        match state {
            State::Media => {
                validate_media(&builder)?;
                media = builder;
            }
            State::Value => add(&mut params, &key, builder),
            State::Key => {
                return Err(einval(
                    ErrorKind::IllegalArgument,
                    format!("no more input found, state=PARAMS_KEY in {uri}"),
                ));
            }
        }
        Ok(Self {
            prefix: prefix.to_string(),
            media,
            params,
        })
    }

    /// The prefix, e.g. [`SPY_QUALIFIER`], or `""`.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Set the prefix (not validated, as in C++).
    pub fn set_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_string();
    }

    /// The media, [`IPC_MEDIA`] or [`UDP_MEDIA`]. (A URI with parameters is
    /// parsed whatever its media, as in C++ and Java.)
    pub fn media(&self) -> &str {
        &self.media
    }

    /// Set the media: [`IPC_MEDIA`] or [`UDP_MEDIA`].
    pub fn set_media(&mut self, media: &str) -> Result<()> {
        validate_media(media)?;
        self.media = media.to_string();
        Ok(())
    }

    /// The scheme, always [`AERON_SCHEME`].
    pub fn scheme(&self) -> &'static str {
        AERON_SCHEME
    }

    /// The value of a parameter.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Set a parameter, replacing its value if it is already set. Not validated,
    /// as in C++: a name or value containing `|`, or an empty value, gives a URI
    /// the media driver rejects (or, for a trailing empty value such as `tags=`,
    /// crashes on).
    pub fn put(&mut self, key: &str, value: &str) {
        match self.params.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value.to_string(),
            None => self.params.push((key.to_string(), value.to_string())),
        }
    }

    /// Remove a parameter, returning its value.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        let index = self.params.iter().position(|(k, _)| k == key)?;
        Some(self.params.remove(index).1)
    }

    /// Returns `true` if the parameter is set.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Returns `true` for a response channel (`control-mode=response`).
    pub fn has_control_mode_response(&self) -> bool {
        self.get(MDC_CONTROL_MODE_PARAM_NAME) == Some(CONTROL_MODE_RESPONSE)
    }

    /// The parameters, in order.
    pub fn params(&self) -> impl Iterator<Item = (&str, &str)> {
        self.params.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// `channel` with its `session-id` set (C++ `ChannelUri::addSessionId`).
    pub fn add_session_id(channel: &str, session_id: i32) -> Result<String> {
        let mut uri = Self::parse(channel)?;
        uri.put(SESSION_ID_PARAM_NAME, &session_id.to_string());
        Ok(uri.to_string())
    }

    /// `uri` with an `alias` added unless it has one or `alias` is empty (C++
    /// `ChannelUri::addAliasIfAbsent`).
    pub fn add_alias_if_absent(uri: &str, alias: &str) -> Result<String> {
        if alias.is_empty() {
            return Ok(uri.to_string());
        }
        let mut parsed = Self::parse(uri)?;
        if parsed.contains_key(ALIAS_PARAM_NAME) {
            return Ok(uri.to_string());
        }
        parsed.put(ALIAS_PARAM_NAME, alias);
        Ok(parsed.to_string())
    }
}

/// A channel URI error, with `EINVAL` as its code like the C++ exceptions.
fn einval(kind: ErrorKind, message: impl Into<String>) -> Error {
    const EINVAL: i32 = 22;
    Error::new(kind, message).with_code(EINVAL)
}

fn validate_media(media: &str) -> Result<()> {
    if media == IPC_MEDIA || media == UDP_MEDIA {
        Ok(())
    } else {
        Err(einval(
            ErrorKind::IllegalArgument,
            format!("unknown media: {media}"),
        ))
    }
}

impl PartialEq for ChannelUri {
    fn eq(&self, other: &Self) -> bool {
        // Keys are unique, so equal lengths and matching lookups mean equal sets.
        self.prefix == other.prefix
            && self.media == other.media
            && self.params.len() == other.params.len()
            && self.params().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl Eq for ChannelUri {}

impl FromStr for ChannelUri {
    type Err = Error;

    fn from_str(uri: &str) -> Result<Self> {
        Self::parse(uri)
    }
}

/// Writes the URI (C++ `toString`): `[prefix:]aeron:media[?key=value|...]`.
impl fmt::Display for ChannelUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.prefix.is_empty() {
            f.write_str(&self.prefix)?;
            if !self.prefix.ends_with(':') {
                f.write_str(":")?;
            }
        }
        write!(f, "{AERON_PREFIX}{}", self.media)?;
        for (i, (key, value)) in self.params.iter().enumerate() {
            write!(f, "{}{key}={value}", if i == 0 { '?' } else { '|' })?;
        }
        Ok(())
    }
}
