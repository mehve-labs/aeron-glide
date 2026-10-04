//! Channel URI building and parsing. The first tests port Aeron's own
//! `ChannelUriStringBuilderTest.cpp` and `ChannelUriTest.java` cases.

use aeron_glide::channel::{self, ChannelUri, ControlMode};
use aeron_glide::{ChannelBuilder, ErrorKind};
use std::time::Duration;

fn udp() -> ChannelBuilder {
    ChannelBuilder::udp().endpoint("localhost:9999")
}

#[test]
fn upstream_builder_cases() {
    assert_eq!(ChannelBuilder::ipc().build().unwrap(), "aeron:ipc");
    assert_eq!(udp().build().unwrap(), "aeron:udp?endpoint=localhost:9999");
    assert_eq!(
        udp().prefix(channel::SPY_QUALIFIER).build().unwrap(),
        "aeron-spy:aeron:udp?endpoint=localhost:9999"
    );
    assert_eq!(
        udp().ttl(9).term_length(1024 * 128).build().unwrap(),
        "aeron:udp?endpoint=localhost:9999|term-length=131072|ttl=9"
    );
    assert_eq!(
        udp()
            .term_length(1024 * 128)
            .initial_term_id(777)
            .term_id(999)
            .term_offset(64)
            .build()
            .unwrap(),
        "aeron:udp?endpoint=localhost:9999|term-length=131072|init-term-id=777|term-id=999|term-offset=64"
    );
    let term_length = 1024 * 128;
    assert_eq!(
        udp()
            .initial_position(i64::from(term_length) * 3 + 64, 777, term_length)
            .build()
            .unwrap(),
        "aeron:udp?endpoint=localhost:9999|term-length=131072|init-term-id=777|term-id=780|term-offset=64"
    );
    assert_eq!(
        udp()
            .socket_sndbuf_length(8192)
            .socket_rcvbuf_length(4096)
            .build()
            .unwrap(),
        "aeron:udp?endpoint=localhost:9999|so-sndbuf=8192|so-rcvbuf=4096"
    );
    assert_eq!(
        udp().receiver_window_length(4096).build().unwrap(),
        "aeron:udp?endpoint=localhost:9999|rcv-wnd=4096"
    );
    assert_eq!(
        udp()
            .media_receive_timestamp_offset("reserved")
            .channel_receive_timestamp_offset("0")
            .channel_send_timestamp_offset("8")
            .build()
            .unwrap(),
        "aeron:udp?endpoint=localhost:9999|media-rcv-ts-offset=reserved|channel-rcv-ts-offset=0|channel-snd-ts-offset=8"
    );
    let uri = ChannelBuilder::udp()
        .endpoint("224.10.9.8:777")
        .max_resend(123)
        .build()
        .unwrap();
    assert!(
        ChannelUri::parse(&uri)
            .unwrap()
            .to_string()
            .contains("max-resend=123")
    );
}

#[test]
fn every_option_in_cpp_order() {
    // Set in reverse order; the output follows the C++ builder's order.
    let uri = ChannelBuilder::udp()
        .max_resend(4)
        .untethered_resting_timeout(Duration::from_millis(2))
        .untethered_window_limit_timeout(Duration::from_millis(1))
        .nak_delay(Duration::from_micros(30))
        .response_correlation_id(42)
        .channel_send_timestamp_offset("8")
        .channel_receive_timestamp_offset("0")
        .media_receive_timestamp_offset("reserved")
        .receiver_window_length(65536)
        .socket_rcvbuf_length(2048)
        .socket_sndbuf_length(1024)
        .spies_simulate_connection(true)
        .rejoin(false)
        .group(true)
        .tether(false)
        .eos(false)
        .sparse(true)
        .group_tag(-7)
        .flow_control("min,t:5s")
        .congestion_control("cubic")
        .alias("orders")
        .linger(Duration::from_secs(1))
        .reliable(false)
        .ttl(8)
        .session_id(-12)
        .term_offset(0)
        .term_id(11)
        .initial_term_id(10)
        .term_length(65536)
        .mtu(1408)
        .control_mode(ControlMode::Manual)
        .control_endpoint("localhost:40457")
        .network_interface("127.0.0.1")
        .endpoint("localhost:40456")
        .tags("1,2")
        .build()
        .unwrap();
    assert_eq!(
        uri,
        "aeron:udp?tags=1,2|endpoint=localhost:40456|interface=127.0.0.1|control=localhost:40457|\
         control-mode=manual|mtu=1408|term-length=65536|init-term-id=10|term-id=11|term-offset=0|\
         session-id=-12|ttl=8|reliable=false|linger=1000000000|alias=orders|cc=cubic|fc=min,t:5s|\
         gtag=-7|sparse=true|eos=false|tether=false|group=true|rejoin=false|ssc=true|so-sndbuf=1024|\
         so-rcvbuf=2048|rcv-wnd=65536|media-rcv-ts-offset=reserved|channel-rcv-ts-offset=0|\
         channel-snd-ts-offset=8|response-correlation-id=42|nak-delay=30000|\
         untethered-window-limit-timeout=1000000|untethered-resting-timeout=2000000|max-resend=4"
    );
    // It parses back to the same parameters.
    let parsed = ChannelUri::parse(&uri).unwrap();
    assert_eq!(parsed.params().count(), 35);
    assert_eq!(parsed.to_string(), uri);
}

#[test]
fn custom_params_follow_known_ones_and_values_are_replaced() {
    let uri = ChannelBuilder::ipc()
        .param("pub-wnd", "65536")
        .alias("first")
        .param("stream-id", "5")
        .alias("second")
        .param("pub-wnd", "131072")
        .build()
        .unwrap();
    assert_eq!(uri, "aeron:ipc?alias=second|pub-wnd=131072|stream-id=5");

    // A generic param replaces a typed one and vice versa.
    let uri = udp().mtu(1408).param("mtu", "4096").build().unwrap();
    assert_eq!(uri, "aeron:udp?endpoint=localhost:9999|mtu=4096");
    let uri = udp()
        .session_id(5)
        .param("session-id", "tag:9")
        .build()
        .unwrap();
    assert_eq!(uri, "aeron:udp?endpoint=localhost:9999|session-id=tag:9");
    let uri = udp()
        .param("session-id", "tag:9")
        .session_id(5)
        .build()
        .unwrap();
    assert_eq!(uri, "aeron:udp?endpoint=localhost:9999|session-id=5");

    // remove() unsets, like the C++ nullptr overloads.
    let uri = udp()
        .reliable(true)
        .rejoin(true)
        .remove("reliable")
        .build()
        .unwrap();
    assert_eq!(uri, "aeron:udp?endpoint=localhost:9999|rejoin=true");
    assert_eq!(
        udp().session_id(1).remove("session-id").build().unwrap(),
        udp().build().unwrap()
    );
    assert_eq!(udp().clear().build().unwrap(), "aeron:udp");
    assert_eq!(
        udp().prefix("aeron-spy").clear().build().unwrap(),
        "aeron:udp"
    );
}

#[test]
fn tagged_session_ids() {
    let uri = udp()
        .session_id(1001)
        .session_id_tagged(true)
        .build()
        .unwrap();
    assert_eq!(uri, "aeron:udp?endpoint=localhost:9999|session-id=tag:1001");
    let uri = udp().session_id_tagged(true).build().unwrap();
    assert_eq!(uri, "aeron:udp?endpoint=localhost:9999");
}

#[test]
fn media_and_prefix() {
    assert_eq!(
        ChannelBuilder::ipc().media("udp").build().unwrap(),
        "aeron:udp"
    );
    assert_eq!(
        ChannelBuilder::ipc().prefix("").build().unwrap(),
        "aeron:ipc"
    );
    let err = ChannelBuilder::ipc().media("tcp").build().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::IllegalArgument);
    let err = ChannelBuilder::ipc()
        .prefix("aeron-sp")
        .build()
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::IllegalArgument);
}

#[test]
fn invalid_values_fail_at_build_with_the_first_error() {
    let invalid = [
        udp().mtu(16),
        udp().mtu(65536),
        udp().mtu(1000),
        udp().term_length(1000),
        udp().term_length(32 * 1024),
        udp().term_length(2 * 1024 * 1024 * 1024),
        udp().term_length(3 * 64 * 1024),
        udp().term_offset(33),
        udp().term_offset(2 * 1024 * 1024 * 1024),
        udp().initial_position(-32, 0, 65536),
        udp().initial_position(33, 0, 65536),
        udp().initial_position(64, 0, 1000),
        udp().linger(Duration::MAX),
        udp().linger(Duration::from_nanos(i64::MAX as u64)),
        udp().nak_delay(Duration::from_secs(u64::MAX)),
        udp().param("bad|key", "x"),
        udp().param("bad=key", "x"),
        udp().param("", "x"),
        udp().param("key", "bad|value"),
        udp().alias("a|b"),
        udp().tags(""),
        udp().param("x", ""),
        udp().endpoint(&"x".repeat(channel::MAX_URI_LENGTH)),
    ];
    for builder in invalid {
        let err = builder.build().expect_err(&format!("{builder:?}"));
        assert!(
            matches!(
                err.kind(),
                ErrorKind::IllegalArgument | ErrorKind::IllegalState
            ),
            "{err}"
        );
    }
    let err = udp().mtu(16).term_length(1000).build().unwrap_err();
    assert!(err.message().contains("MTU"), "{err}");
    assert_eq!(err.code(), 22, "EINVAL, as in C++");
    // clear() keeps an earlier invalid setting.
    assert!(udp().mtu(1000).clear().build().is_err());
    assert!(ChannelBuilder::ipc().prefix("x").clear().build().is_err());
    // Valid boundaries.
    assert!(udp().mtu(32).mtu(65504).build().is_ok());
    assert!(
        udp()
            .term_length(64 * 1024)
            .term_length(1 << 30)
            .build()
            .is_ok()
    );
    assert!(udp().term_offset(1 << 30).build().is_ok());
    assert!(udp().linger(Duration::ZERO).build().is_ok());
}

#[test]
fn initial_position_computes_term_id_and_offset() {
    let uri = ChannelBuilder::ipc()
        .initial_position(0, -5, 65536)
        .build()
        .unwrap();
    assert_eq!(
        uri,
        "aeron:ipc?term-length=65536|init-term-id=-5|term-id=-5|term-offset=0"
    );
    // Term IDs wrap.
    let uri = ChannelBuilder::ipc()
        .initial_position(65536 * 2 + 96, i32::MAX, 65536)
        .build()
        .unwrap();
    assert!(
        uri.contains(&format!("term-id={}", i32::MAX.wrapping_add(2))),
        "{uri}"
    );
    assert!(uri.ends_with("term-offset=96"), "{uri}");
}

#[test]
fn upstream_parse_cases() {
    for (uri, prefix, media) in [
        ("aeron:udp", "", "udp"),
        ("aeron:ipc", "", "ipc"),
        ("aeron-spy:aeron:ipc", "aeron-spy", "ipc"),
    ] {
        let parsed = ChannelUri::parse(uri).unwrap();
        assert_eq!(parsed.prefix(), prefix);
        assert_eq!(parsed.media(), media);
        assert_eq!(parsed.scheme(), "aeron");
        assert_eq!(parsed.params().count(), 0);
        assert_eq!(parsed.to_string(), uri);
    }
    for invalid in [
        ":udp",
        "aeron",
        "aron:",
        "eeron:",
        "aeron:udp:",
        "aeron:ipcsdfgfdhfgf",
        "aeron:ipc|sparse=true",
        "aeron:udp?endpoint=localhost:4652|-~@{]|=??#s!£$%====",
        "aeron:",
        "aeron:udp?",
        "aeron:udp?endpoint=x|",
        "aeron:udp?=x",
        "aeron:udp?endpoint",
        "",
    ] {
        let err = ChannelUri::parse(invalid).expect_err(invalid);
        assert!(
            matches!(
                err.kind(),
                ErrorKind::IllegalArgument | ErrorKind::IllegalState
            ),
            "{invalid}: {err}"
        );
    }

    let uri =
        ChannelUri::parse("aeron:udp?endpoint=224.10.9.8|port=4567|interface=192.168.0.3|ttl=16")
            .unwrap();
    assert_eq!(uri.get("endpoint"), Some("224.10.9.8"));
    assert_eq!(uri.get("port"), Some("4567"));
    assert_eq!(uri.get("interface"), Some("192.168.0.3"));
    assert_eq!(uri.get("ttl"), Some("16"));
    assert_eq!(uri.get("mtu"), None);
    assert_eq!(uri.get("mtu").unwrap_or("1408"), "1408");
}

#[test]
fn parse_edge_cases() {
    // Values may contain '=', ':' and '?'; keys may not be empty.
    let uri = ChannelUri::parse("aeron:udp?fc=tagged,g:1/2|endpoint=a:1|x=a=b?c").unwrap();
    assert_eq!(uri.get("fc"), Some("tagged,g:1/2"));
    assert_eq!(uri.get("x"), Some("a=b?c"));
    // An empty value parses, as in C++ (the media driver rejects it, or crashes
    // on a trailing `tags=`, so the builder refuses to write one).
    assert_eq!(
        ChannelUri::parse("aeron:udp?alias=").unwrap().get("alias"),
        Some("")
    );
    // A repeated key keeps its first value, as in C++.
    let uri = ChannelUri::parse("aeron:udp?mtu=1|mtu=2").unwrap();
    assert_eq!(uri.get("mtu"), Some("1"));
    assert_eq!(uri.to_string(), "aeron:udp?mtu=1");
    // As in C++ and Java, the media is only checked when there are no parameters.
    assert_eq!(ChannelUri::parse("aeron:tcp?a=1").unwrap().media(), "tcp");
    // Non-ASCII characters are kept.
    let uri = ChannelUri::parse("aeron:ipc?alias=café|tags=1").unwrap();
    assert_eq!(uri.get("alias"), Some("café"));
    let err = ChannelUri::parse("aeron:ipc?é|x").unwrap_err();
    // Indexes are byte offsets, as in C++.
    assert!(err.message().contains("index 12"), "{err}");
}

#[test]
fn modify_and_write_back() {
    let mut uri: ChannelUri = "aeron:udp?endpoint=localhost:1|mtu=1408".parse().unwrap();
    uri.put("mtu", "4096");
    uri.put("alias", "x");
    assert_eq!(
        uri.to_string(),
        "aeron:udp?endpoint=localhost:1|mtu=4096|alias=x"
    );
    assert_eq!(uri.remove("endpoint").as_deref(), Some("localhost:1"));
    assert_eq!(uri.remove("endpoint"), None);
    assert!(uri.contains_key("alias"));
    assert!(!uri.contains_key("endpoint"));
    uri.set_media("ipc").unwrap();
    assert_eq!(
        uri.set_media("tcp").unwrap_err().kind(),
        ErrorKind::IllegalArgument
    );
    uri.set_prefix("aeron-spy");
    assert_eq!(uri.to_string(), "aeron-spy:aeron:ipc?mtu=4096|alias=x");
    uri.set_prefix("aeron-spy:");
    assert_eq!(uri.to_string(), "aeron-spy:aeron:ipc?mtu=4096|alias=x");
    assert_eq!(
        uri.params().collect::<Vec<_>>(),
        [("mtu", "4096"), ("alias", "x")]
    );
}

#[test]
fn equality_ignores_parameter_order() {
    let a = ChannelUri::parse("aeron:udp?endpoint=a:1|mtu=1408").unwrap();
    let b = ChannelUri::parse("aeron:udp?mtu=1408|endpoint=a:1").unwrap();
    assert_eq!(a, b);
    for different in [
        "aeron:udp?endpoint=a:1",
        "aeron:udp?endpoint=a:1|mtu=1409",
        "aeron:udp?endpoint=a:1|mtu=1408|ttl=1",
        "aeron:ipc?endpoint=a:1|mtu=1408",
        "aeron-spy:aeron:udp?endpoint=a:1|mtu=1408",
    ] {
        assert_ne!(a, ChannelUri::parse(different).unwrap(), "{different}");
    }
}

#[test]
fn response_control_mode() {
    let uri = ChannelBuilder::udp()
        .control_mode(ControlMode::Response)
        .control_endpoint("localhost:1")
        .build()
        .unwrap();
    assert!(ChannelUri::parse(&uri).unwrap().has_control_mode_response());
    assert!(
        !ChannelUri::parse("aeron:udp?control-mode=dynamic")
            .unwrap()
            .has_control_mode_response()
    );
    assert!(
        !ChannelUri::parse("aeron:ipc")
            .unwrap()
            .has_control_mode_response()
    );
    assert_eq!(ControlMode::Response.to_string(), "response");
    for mode in [
        ControlMode::Manual,
        ControlMode::Dynamic,
        ControlMode::Response,
    ] {
        assert_eq!(mode.to_string().parse::<ControlMode>().unwrap(), mode);
    }
    assert!("Manual".parse::<ControlMode>().is_err());
}

#[test]
fn add_session_id_and_alias() {
    assert_eq!(
        ChannelUri::add_session_id("aeron:ipc", 7).unwrap(),
        "aeron:ipc?session-id=7"
    );
    assert_eq!(
        ChannelUri::add_session_id("aeron:udp?session-id=1|mtu=1408", -7).unwrap(),
        "aeron:udp?session-id=-7|mtu=1408"
    );
    assert!(ChannelUri::add_session_id("udp", 7).is_err());

    // Unchanged (not even parsed) when the alias is empty or already set.
    assert_eq!(
        ChannelUri::add_alias_if_absent("not a uri", "").unwrap(),
        "not a uri"
    );
    assert_eq!(
        ChannelUri::add_alias_if_absent("aeron:ipc?alias=a", "b").unwrap(),
        "aeron:ipc?alias=a"
    );
    assert_eq!(
        ChannelUri::add_alias_if_absent("aeron:udp?endpoint=a:1", "b").unwrap(),
        "aeron:udp?endpoint=a:1|alias=b"
    );
    assert!(ChannelUri::add_alias_if_absent("not a uri", "b").is_err());
}

#[test]
fn window_linger_stream_and_ats_parameters() {
    let uri = ChannelBuilder::udp()
        .endpoint("localhost:20121")
        .publication_window_length(65536)
        .untethered_linger_timeout(Duration::from_millis(5))
        .stream_id(7)
        .ats(true)
        .build()
        .unwrap();
    let parsed = ChannelUri::parse(&uri).unwrap();
    assert_eq!(parsed.get("pub-wnd"), Some("65536"));
    assert_eq!(parsed.get("untethered-linger-timeout"), Some("5000000"));
    assert_eq!(parsed.get("stream-id"), Some("7"));
    assert_eq!(parsed.get("ats"), Some("true"));
}
